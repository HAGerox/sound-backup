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

test('removing QLab forgets the saved login and clears its selection', async () => {
  const calls = [];
  const context = frontend(async (command, arguments_) => { calls.push([command, arguments_]); });
  vm.runInContext("settings.devices.qlab.selectedWorkspaces = [{ id: 'w1', name: 'Show', port: 53000, version: '5' }]", context);
  await vm.runInContext("removeConnection('qlab')", context);
  assert.equal(calls[0][0], 'forget_remote_access');
  assert.equal(calls[0][1].remote.username, 'operator');
  assert.equal(vm.runInContext('settings.devices.qlab.instance.host', context), '');
  assert.equal(vm.runInContext('settings.devices.qlab.selectedWorkspaces.length', context), 0);
  assert.equal(vm.runInContext('states.qlab', context), 'not-configured');
  assert.equal(calls.at(-1)[0], 'save_settings');
});

test('removing an Avantis clears its address and selected Shows', async () => {
  const commands = [];
  const context = frontend(async (command) => { commands.push(command); });
  vm.runInContext(`
    settings.devices.avantis = { endpoint: '192.168.1.109', selectedShows: ['Sunday'] };
    availableShows = [{ name: 'Sunday' }];
  `, context);
  await vm.runInContext("removeConnection('avantis')", context);
  assert.equal(vm.runInContext('settings.devices.avantis.endpoint', context), '');
  assert.equal(vm.runInContext('settings.devices.avantis.selectedShows.length', context), 0);
  assert.equal(vm.runInContext('availableShows.length', context), 0);
  assert.deepEqual(commands, ['save_settings']);
});

test('a failed Show refresh reports the console error instead of a generic message', async () => {
  const context = frontend(async (command) => {
    if (command === 'load_avantis_shows') throw 'Timed out during the AH-Net connection handshake.';
  });
  vm.runInContext("settings.devices.avantis.endpoint = '192.168.1.109'", context);
  await vm.runInContext('refreshShows({ silent: true })', context);
  assert.equal(vm.runInContext('elements.showStatus.textContent', context), 'Timed out during the AH-Net connection handshake.');
  assert.equal(vm.runInContext('states.avantis', context), 'offline');
});


test('a failed manual QLab connection preserves the saved host and login', async () => {
  const context = frontend(async () => { throw 'QLab did not respond'; });
  vm.runInContext("elements.qlabAddress.value = '10.10.160.121'", context);
  await vm.runInContext('connectQLabAddress()', context);
  assert.equal(vm.runInContext('settings.devices.qlab.instance.host', context), 'qlab.example');
  assert.equal(vm.runInContext('settings.devices.qlab.remote.fingerprint', context), 'pinned-host');
  assert.equal(vm.runInContext('elements.qlabDiscoveryStatus.textContent', context), 'QLab did not respond');
  assert.equal(vm.runInContext('elements.qlabManualConnectButton.disabled', context), false);
});

test('manual QLab connection lists the remote workspace and requests file access', async () => {
  const context = frontend(async command => {
    if (command === 'load_qlab_workspaces') return [{ id: 'w1', name: 'Show', port: 53000, version: '5' }];
  });
  vm.runInContext("elements.qlabAddress.value = '10.10.160.121'", context);
  await vm.runInContext('connectQLabAddress()', context);
  assert.equal(vm.runInContext('settings.devices.qlab.instance.host', context), '10.10.160.121');
  assert.equal(vm.runInContext('settings.devices.qlab.instance.local', context), false);
  assert.equal(vm.runInContext('settings.devices.qlab.remote.fingerprint', context), '');
  assert.equal(vm.runInContext('settings.devices.qlab.selectedWorkspaces[0].name', context), 'Show');
  assert.equal(vm.runInContext('elements.qlabLogin.hidden', context), false);
});
