import './third-party-notices.mjs';
import { spawnSync } from 'node:child_process';
import { cp, mkdir } from 'node:fs/promises';
import { homedir } from 'node:os';
import { resolve } from 'node:path';

// Panic locations and vendored OpenSSL installation paths must not disclose
// the builder's home directory. Keep native build output outside that home.
const flags = `${process.env.RUSTFLAGS || ''} --remap-path-prefix=${homedir()}=/build`.trim();
const buildTarget = '/tmp/sound-backup-release';
const result = spawnSync('npm', ['run', 'tauri', '--', 'build', '--bundles', 'app,dmg'], {
  stdio: 'inherit',
  env: { ...process.env, RUSTFLAGS: flags, CARGO_TARGET_DIR: buildTarget },
});
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const output = resolve(import.meta.dirname, '../target/release/bundle');
await mkdir(output, { recursive: true });
await cp(`${buildTarget}/release/bundle`, output, { recursive: true, force: true });
