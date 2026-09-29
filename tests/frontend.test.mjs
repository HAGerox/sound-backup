import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import test from 'node:test';

const source = await readFile(new URL('../src-web/app.js', import.meta.url), 'utf8');

function frontend(invoke) {
  const nodes = new Map();
  const document = {
    querySelector(selector) {
      if (!nodes.has(selector)) nodes.set(selector, {
        value: '', hidden: false, dataset: {},
        classList: { toggle() {} },
        addEventListener() {}, setAttribute() {}, replaceChildren() {}, append() {},
      });
      return nodes.get(selector);
    },
    addEventListener() {},
    createElement() { return { classList: { toggle() {} }, addEventListener() {}, append() {} }; },
  };
  const context = vm.createContext({
    window: { __TAURI__: { core: { invoke } } },
    document, setTimeout: () => 1, clearTimeout() {}, URL,
  });
  // Startup uses live device discovery; the test drives the same UI handler directly.
  vm.runInContext(source.replace(/listenToBackend\(\);\s*load\(\);\s*$/, ''), context);
  vm.runInContext(`
    settings.devices.qlab.instance.host = 'qlab.example';
    settings.devices.qlab.remote = {
      host: 'qlab.example', username: 'operator', fingerprint: 'pinned-host', local: false
    };
    elements.qlabUsername.value = 'operator';
  `, context);
  return context;
}

test('a failed SSH retry retains the pinned host identity', async () => {
  const context = frontend(async () => { throw new Error('The remote Mac’s identity has changed.'); });
  await vm.runInContext('connectQLabRemote({ silent: true })', context);
  assert.equal(vm.runInContext('settings.devices.qlab.remote.fingerprint', context), 'pinned-host');
  assert.equal(vm.runInContext('states.qlab', context), 'offline');
});

test('a successful SSH connection saves the returned host identity and clears the password', async () => {
  const commands = [];
  const context = frontend(async (command) => {
    commands.push(command);
    return command === 'test_remote_access' ? { fingerprint: 'verified-host' } : undefined;
  });
  vm.runInContext("elements.qlabPassword.value = 'test-password'", context);
  await vm.runInContext('connectQLabRemote({ usePassword: true })', context);
  assert.equal(vm.runInContext('settings.devices.qlab.remote.fingerprint', context), 'verified-host');
  assert.equal(vm.runInContext('elements.qlabPassword.value', context), '');
  assert.deepEqual(commands, ['test_remote_access', 'save_settings']);
});
