use keyring::Entry;
use ssh2::{HashType, KeyboardInteractivePrompt, Prompt, Session};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::Path,
    time::Duration,
};

const REMOTE_LOGIN_SERVICE: &str = "uk.stagebackup.remote-login";

#[derive(Clone, Debug, Default)]
pub struct Connection {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub fingerprint: String,
    pub local: bool,
}

#[derive(Clone)]
pub struct Connected {
    pub session: Session,
    pub fingerprint: String,
}

pub fn connect(
    connection: &Connection,
    supplied_password: Option<&str>,
) -> Result<Connected, String> {
    if connection.local {
        return Err("This Mac does not need Remote Login.".to_string());
    }
    if connection.host.trim().is_empty() || connection.username.trim().is_empty() {
        return Err("Choose a Mac and enter its macOS account name.".to_string());
    }
    let port = if connection.port == 0 {
        22
    } else {
        connection.port
    };
    let address = resolve(&connection.host, port)?;
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(5)).map_err(|error| {
        format!(
            "Could not reach Remote Login on {}: {error}",
            connection.host
        )
    })?;
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(15)));

    let mut session = Session::new().map_err(|error| format!("Could not start SSH: {error}"))?;
    session.set_timeout(15_000);
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|error| format!("Remote Login handshake failed: {error}"))?;
    let fingerprint = session
        .host_key_hash(HashType::Sha256)
        .map(hex)
        .ok_or_else(|| "The remote Mac did not provide a host fingerprint.".to_string())?;
    if !connection.fingerprint.is_empty() && connection.fingerprint != fingerprint {
        return Err("The remote Mac’s identity has changed. Remove it and set it up again before backing up.".to_string());
    }

    let username = connection.username.trim();
    let _ = session.userauth_agent(username);
    if !session.authenticated() {
        let password = match supplied_password.filter(|password| !password.is_empty()) {
            Some(password) => Some(password.to_string()),
            None => load_password(connection)?,
        };
        if let Some(password) = password {
            let password_result = session.userauth_password(username, &password);
            if password_result.is_err() || !session.authenticated() {
                let mut prompt = PasswordPrompt {
                    password: &password,
                };
                let _ = session.userauth_keyboard_interactive(username, &mut prompt);
            }
        }
    }
    if !session.authenticated() {
        return Err("Could not sign in. Check the account name and password, and make sure Remote Login is enabled on that Mac.".to_string());
    }
    if let Some(password) = supplied_password.filter(|password| !password.is_empty()) {
        save_password(connection, password)?;
    }
    Ok(Connected {
        session,
        fingerprint,
    })
}

pub fn run(session: &Session, command: &str) -> Result<String, String> {
    let mut channel = session
        .channel_session()
        .map_err(|error| format!("Could not open a Remote Login command: {error}"))?;
    channel
        .exec(command)
        .map_err(|error| format!("Could not run a command on the remote Mac: {error}"))?;
    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|error| format!("Could not read the remote Mac’s response: {error}"))?;
    let mut error_output = String::new();
    channel
        .stderr()
        .read_to_string(&mut error_output)
        .map_err(|error| format!("Could not read the remote Mac’s error response: {error}"))?;
    channel
        .wait_close()
        .map_err(|error| format!("The remote command did not close cleanly: {error}"))?;
    let status = channel
        .exit_status()
        .map_err(|error| format!("Could not read the remote command status: {error}"))?;
    if status != 0 {
        let detail = error_output.trim();
        return Err(if detail.is_empty() {
            format!("The remote Mac could not complete the command (status {status}).")
        } else {
            detail.to_string()
        });
    }
    Ok(output)
}

pub fn download(session: &Session, remote_path: &str, local_path: &Path) -> Result<u64, String> {
    let sftp = session
        .sftp()
        .map_err(|error| format!("Could not start the file transfer: {error}"))?;
    let mut source = sftp
        .open(Path::new(remote_path))
        .map_err(|error| format!("Could not open the prepared backup: {error}"))?;
    let mut destination = std::fs::File::create(local_path)
        .map_err(|error| format!("Could not create the local backup: {error}"))?;
    let bytes = std::io::copy(&mut source, &mut destination)
        .map_err(|error| format!("The backup transfer stopped: {error}"))?;
    destination
        .flush()
        .map_err(|error| format!("Could not finish writing the backup: {error}"))?;
    Ok(bytes)
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("Could not resolve {host}: {error}"))?
        .find(|address| address.is_ipv4())
        .or_else(|| (host, port).to_socket_addrs().ok()?.next())
        .ok_or_else(|| format!("Could not find a network address for {host}."))
}

fn password_account(connection: &Connection) -> String {
    format!(
        "{}@{}:{}",
        connection.username.trim(),
        connection.host.trim().to_ascii_lowercase(),
        if connection.port == 0 {
            22
        } else {
            connection.port
        }
    )
}

fn save_password(connection: &Connection, password: &str) -> Result<(), String> {
    Entry::new(REMOTE_LOGIN_SERVICE, &password_account(connection))
        .map_err(|error| format!("Could not open Keychain: {error}"))?
        .set_password(password)
        .map_err(|error| format!("Could not save the password in Keychain: {error}"))
}

fn load_password(connection: &Connection) -> Result<Option<String>, String> {
    let entry = Entry::new(REMOTE_LOGIN_SERVICE, &password_account(connection))
        .map_err(|error| format!("Could not open Keychain: {error}"))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Could not read the saved password from Keychain: {error}"
        )),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct PasswordPrompt<'a> {
    password: &'a str,
}

impl KeyboardInteractivePrompt for PasswordPrompt<'_> {
    fn prompt<'a>(
        &mut self,
        _username: &str,
        _instructions: &str,
        prompts: &[Prompt<'a>],
    ) -> Vec<String> {
        prompts.iter().map(|_| self.password.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_stable_keychain_accounts() {
        let connection = Connection {
            host: "QLab-Mac.local".to_string(),
            port: 0,
            username: " finn ".to_string(),
            ..Connection::default()
        };
        assert_eq!(password_account(&connection), "finn@qlab-mac.local:22");
    }
}
