const nativeInvoke = window.__TAURI__?.core?.invoke;

async function invoke(command, arguments_) {
  if (nativeInvoke) return nativeInvoke(command, arguments_);
  if (command === 'load_settings') return {};
  if (command === 'save_settings') return;
  throw new Error('This action is available in the Stage Backup desktop app.');
}

const elements = {
  form: document.querySelector('#backupForm'),
  consoleAddress: document.querySelector('#consoleAddress'),
  showName: document.querySelector('#showName'),
  backupFolder: document.querySelector('#backupFolder'),
  chooseFolderButton: document.querySelector('#chooseFolderButton'),
  backupButton: document.querySelector('#backupButton'),
  testButton: document.querySelector('#testButton'),
  status: document.querySelector('#status'),
  statusText: document.querySelector('#statusText'),
  statusPath: document.querySelector('#statusPath')
};

let saveTimer;
let busy = false;

function settings() {
  return {
    consoleAddress: elements.consoleAddress.value.trim(),
    showName: elements.showName.value.trim(),
    backupFolder: elements.backupFolder.value.trim()
  };
}

function setStatus(text, kind = '', path = '') {
  elements.status.dataset.kind = kind;
  elements.statusText.textContent = text;
  elements.statusPath.textContent = path;
}

function setBusy(value, label = 'Back up show') {
  busy = value;
  elements.consoleAddress.disabled = value;
  elements.showName.disabled = value;
  elements.chooseFolderButton.disabled = value;
  elements.testButton.disabled = value;
  elements.backupButton.disabled = value;
  elements.backupButton.textContent = value ? 'Backing up…' : label;
}

async function saveSettingsSoon() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    saveTimer = undefined;
    try {
      await invoke('save_settings', { settings: settings() });
    } catch (error) {
      setStatus(String(error), 'error');
    }
  }, 250);
}

function validate({ consoleAddress, showName, backupFolder }, requireFolder = true) {
  if (!consoleAddress) return 'Enter the console address.';
  if (!showName && requireFolder) return 'Enter the stored Show name.';
  if (showName && new TextEncoder().encode(showName).length > 16) return 'Avantis Show names are limited to 16 bytes.';
  if (requireFolder && !backupFolder) return 'Choose a backup folder.';
  return '';
}

async function load() {
  try {
    const stored = await invoke('load_settings');
    elements.consoleAddress.value = stored.consoleAddress || '';
    elements.showName.value = stored.showName || '';
    elements.backupFolder.value = stored.backupFolder || '';
  } catch (error) {
    setStatus(String(error), 'error');
  }
}

for (const input of [elements.consoleAddress, elements.showName]) {
  input.addEventListener('input', saveSettingsSoon);
}

elements.chooseFolderButton.addEventListener('click', async () => {
  if (busy) return;
  try {
    const selected = await invoke('choose_backup_folder');
    if (selected) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      elements.backupFolder.value = selected;
      await invoke('save_settings', { settings: settings() });
      setStatus('Ready.');
    }
  } catch (error) {
    setStatus(String(error), 'error');
  }
});

elements.testButton.addEventListener('click', async () => {
  if (busy) return;
  const current = settings();
  const issue = validate(current, false);
  if (issue) {
    setStatus(issue, 'error');
    return;
  }
  setBusy(true);
  setStatus('Connecting…');
  try {
    await invoke('test_avantis', { consoleAddress: current.consoleAddress });
    setStatus('Avantis connected.', 'success');
  } catch (error) {
    setStatus(String(error), 'error');
  } finally {
    setBusy(false);
  }
});

elements.form.addEventListener('submit', async (event) => {
  event.preventDefault();
  if (busy) return;
  const current = settings();
  const issue = validate(current, true);
  if (issue) {
    setStatus(issue, 'error');
    return;
  }

  setBusy(true);
  setStatus('Connecting and backing up the stored Show…');
  try {
    clearTimeout(saveTimer);
    saveTimer = undefined;
    await invoke('save_settings', { settings: current });
    const result = await invoke('backup_avantis_show', { input: current });
    setStatus(`Backed up ${result.showName}.`, 'success', result.path);
  } catch (error) {
    setStatus(String(error), 'error');
  } finally {
    setBusy(false);
  }
});

load();
