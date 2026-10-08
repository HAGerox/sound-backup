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
        value: '', hidden: false, dataset: {}, open: false,
        classList: { toggle() {} },
        addEventListener() {}, setAttribute() {}, replaceChildren() {}, append() {},
        showModal() { this.open = true; }, close() { this.open = false; },
      });
      return nodes.get(selector);
    },
    addEventListener() {},
    createElement() { return { classList: { toggle() {} }, addEventListener() {}, setAttribute() {}, append() {} }; },
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

// Naming patterns from a real Avantis User Show list, 2026-10-08 (abridged).
const DESK_SHOWS = [
  '2026 Addams v4', '2026 Addams v9', '2026 Addams v10', '2026 Addams v11', '2026 Addams v12',
  '2026 GnD Tech v7', '2026 GnD Tech v8', 'GnD Tech v8', 'Fringe 2025 v9', 'Fringe 2025 v15',
  'Fringe v13', 'Fun Home 26', 'Overture 25 Pre ', 'SOUND TRAINING'
];

function avantisDesk(shows, extra = () => {}) {
  const desk = { shows: [...shows], saved: [], backups: [] };
  desk.invoke = async (command, arguments_) => {
    if (command === 'load_avantis_shows') return desk.shows.map(name => ({ name }));
    if (command === 'save_settings') { desk.saved.push(JSON.parse(JSON.stringify(arguments_.settings ?? arguments_))); return; }
    if (command === 'backup_avantis_shows') { desk.backups.push([...arguments_.input.showNames]); return { files: arguments_.input.showNames.map(() => ({})) }; }
    return extra(command, arguments_);
  };
  return desk;
}

function withSelection(context, selected, declined = []) {
  vm.runInContext(`settings.devices.avantis = { endpoint: '192.168.1.109', selectedShows: ${JSON.stringify(selected)}, declinedNewerShows: ${JSON.stringify(declined)} };`, context);
}

async function until(check) {
  for (let attempt = 0; attempt < 500; attempt += 1) {
    if (check()) return;
    await new Promise(resolve => setImmediate(resolve));
  }
  throw new Error('Timed out waiting for the frontend.');
}

const dialogOpen = context => vm.runInContext('elements.newerShowDialog.open', context);
const selection = context => JSON.parse(vm.runInContext('JSON.stringify(selectedShows())', context));

test('Show versions compare numerically within the same Show name', () => {
  const context = frontend(async () => {});
  vm.runInContext(`availableShows = ${JSON.stringify(DESK_SHOWS.map(name => ({ name })))};`, context);
  const offers = names => JSON.parse(vm.runInContext(`JSON.stringify(newerShowVersions(${JSON.stringify(names)}))`, context));

  withSelection(context, ['2026 Addams v9']);
  assert.deepEqual(offers(['2026 Addams v9']), [{ current: '2026 Addams v9', newer: '2026 Addams v12' }]);
  // Different prefixes are different Shows, even when the rest of the name matches.
  withSelection(context, ['GnD Tech v8', 'Fringe v13']);
  assert.deepEqual(offers(['GnD Tech v8', 'Fringe v13']), []);
  withSelection(context, ['Fringe 2025 v9', 'Fun Home 26']);
  assert.deepEqual(offers(['Fringe 2025 v9', 'Fun Home 26']), [{ current: 'Fringe 2025 v9', newer: 'Fringe 2025 v15' }]);
  // Already on the newest, or newest already selected alongside: nothing to ask.
  withSelection(context, ['2026 Addams v12']);
  assert.deepEqual(offers(['2026 Addams v12']), []);
  withSelection(context, ['2026 Addams v11', '2026 Addams v12']);
  assert.deepEqual(offers(['2026 Addams v11', '2026 Addams v12']), []);
  // Case and separators don't matter.
  vm.runInContext("availableShows = [{ name: 'Panto_V3' }, { name: 'panto v2' }];", context);
  withSelection(context, ['panto v2']);
  assert.deepEqual(offers(['panto v2']), [{ current: 'panto v2', newer: 'Panto_V3' }]);
});

test('loading Shows offers the newer version and switching replaces the selection', async () => {
  const desk = avantisDesk(DESK_SHOWS);
  const context = frontend(desk.invoke);
  withSelection(context, ['2026 Addams v11', 'Fun Home 26']);
  const refreshed = vm.runInContext('refreshShows({ silent: true })', context);
  await until(() => dialogOpen(context));
  assert.equal(vm.runInContext('elements.newerShowUseButton.textContent', context), 'Use 2026 Addams v12');
  assert.equal(vm.runInContext('elements.newerShowKeepButton.textContent', context), 'Keep 2026 Addams v11');
  vm.runInContext('answerNewerShowDialog(true)', context);
  assert.equal(await refreshed, true);
  assert.equal(dialogOpen(context), false);
  assert.deepEqual(selection(context), ['Fun Home 26', '2026 Addams v12']);
  assert.deepEqual(desk.saved.at(-1).devices.avantis.selectedShows, ['Fun Home 26', '2026 Addams v12']);
});

test('keeping the older Show is remembered until a later version appears', async () => {
  const desk = avantisDesk(DESK_SHOWS);
  const context = frontend(desk.invoke);
  withSelection(context, ['2026 Addams v11']);
  const first = vm.runInContext('refreshShows({ silent: true })', context);
  await until(() => dialogOpen(context));
  vm.runInContext('answerNewerShowDialog(false)', context);
  assert.equal(await first, false);
  assert.deepEqual(selection(context), ['2026 Addams v11']);
  assert.deepEqual(desk.saved.at(-1).devices.avantis.declinedNewerShows, ['2026 Addams v12']);

  // Same desk again: no prompt.
  assert.equal(await vm.runInContext('refreshShows({ silent: true })', context), false);
  assert.equal(dialogOpen(context), false);

  // v13 is stored: ask again.
  desk.shows.push('2026 Addams v13');
  const third = vm.runInContext('refreshShows({ silent: true })', context);
  await until(() => dialogOpen(context));
  assert.equal(vm.runInContext('elements.newerShowUseButton.textContent', context), 'Use 2026 Addams v13');
  vm.runInContext('answerNewerShowDialog(true)', context);
  await third;
  assert.deepEqual(selection(context), ['2026 Addams v13']);
});

test('a newer version is still offered when the selected Show was deleted from the desk', async () => {
  const desk = avantisDesk(DESK_SHOWS.filter(name => name !== '2026 Addams v11'));
  const context = frontend(desk.invoke);
  withSelection(context, ['2026 Addams v11']);
  const refreshed = vm.runInContext('refreshShows({ silent: true })', context);
  await until(() => dialogOpen(context));
  vm.runInContext('answerNewerShowDialog(true)', context);
  await refreshed;
  assert.deepEqual(selection(context), ['2026 Addams v12']);
});

test('Back Up re-reads the desk and backs up the newer Show once accepted', async () => {
  const desk = avantisDesk(['2026 Addams v11']);
  const context = frontend(desk.invoke);
  vm.runInContext("settings.backupFolder = '/Volumes/Backups/Sound'", context);
  withSelection(context, ['2026 Addams v11']);
  await vm.runInContext('refreshShows({ silent: true })', context);
  assert.equal(dialogOpen(context), false);

  desk.shows.push('2026 Addams v12'); // Stored on the desk while the app was open.
  const backup = vm.runInContext('runBackup()', context);
  await until(() => dialogOpen(context));
  assert.equal(desk.backups.length, 0, 'nothing is backed up while the question is open');
  vm.runInContext('answerNewerShowDialog(true)', context);
  await backup;
  assert.deepEqual(desk.backups, [['2026 Addams v12']]);
});

test('Back Up keeps the older Show when the newer one is declined', async () => {
  const desk = avantisDesk(DESK_SHOWS);
  const context = frontend(desk.invoke);
  vm.runInContext("settings.backupFolder = '/Volumes/Backups/Sound'", context);
  withSelection(context, ['2026 Addams v11']);
  const backup = vm.runInContext('runBackup()', context);
  await until(() => dialogOpen(context));
  vm.runInContext('answerNewerShowDialog(false)', context);
  await backup;
  assert.deepEqual(desk.backups, [['2026 Addams v11']]);
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
