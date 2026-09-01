const nativeInvoke = window.__TAURI__?.core?.invoke;

const emptyRemote = () => ({
  host: '', name: '', port: 22, username: '', fingerprint: '', local: false
});

const emptySettings = () => ({
  backupFolder: '',
  devices: {
    avantis: { endpoint: '', selectedShows: [] },
    qlab: {
      instance: { host: '', address: '', hostname: '', name: '', oscPort: 53000, local: false },
      remote: emptyRemote(),
      selectedWorkspaces: []
    }
  }
});

async function invoke(command, arguments_) {
  if (nativeInvoke) return nativeInvoke(command, arguments_);
  if (command === 'load_settings') return emptySettings();
  if (command === 'save_settings') return;
  if (command === 'open_remote_login_settings') return;
  if (command.startsWith('discover_') || command.startsWith('load_')) return [];
  throw new Error('This action is available in the Stage Backup desktop app.');
}

const $ = selector => document.querySelector(selector);
const elements = {
  homeView: $('#homeView'), settingsView: $('#settingsView'), settingsButton: $('#settingsButton'),
  settingsBackButton: $('#settingsBackButton'), backupButton: $('#backupButton'),
  backupButtonIcon: $('#backupButtonIcon'), backupButtonLabel: $('#backupButtonLabel'),
  backupSummary: $('#backupSummary'), progressPanel: $('#progressPanel'),
  progressTitle: $('#progressTitle'), progressDetail: $('#progressDetail'),
  backupFolderDisplay: $('#backupFolderDisplay'), chooseFolderButton: $('#chooseFolderButton'),
  avantisIndicator: $('#avantisIndicator'), avantisIndicatorStatus: $('#avantisIndicatorStatus'),
  avantisSettingsCard: $('#avantisSettingsCard'),
  discoverButton: $('#discoverButton'), discoverButtonLabel: $('#discoverButtonLabel'),
  discoveryStatus: $('#discoveryStatus'), deviceChoices: $('#deviceChoices'),
  manualAddress: $('#manualAddress'), manualConnectButton: $('#manualConnectButton'),
  showSettings: $('#showSettings'), refreshShowsButton: $('#refreshShowsButton'),
  showToolbar: $('#showToolbar'), showSearch: $('#showSearch'),
  selectAllShowsButton: $('#selectAllShowsButton'), showStatus: $('#showStatus'),
  showChoices: $('#showChoices'), showCountText: $('#showCountText'),
  qlabIndicator: $('#qlabIndicator'), qlabIndicatorStatus: $('#qlabIndicatorStatus'),
  qlabSettingsCard: $('#qlabSettingsCard'), qlabDiscoverButton: $('#qlabDiscoverButton'),
  qlabDiscoverButtonLabel: $('#qlabDiscoverButtonLabel'), qlabDiscoveryStatus: $('#qlabDiscoveryStatus'),
  qlabInstanceChoices: $('#qlabInstanceChoices'), qlabWorkspaceStatus: $('#qlabWorkspaceStatus'),
  qlabPasscodePanel: $('#qlabPasscodePanel'), qlabPasscode: $('#qlabPasscode'),
  qlabUnlockButton: $('#qlabUnlockButton'), qlabLogin: $('#qlabLogin'),
  qlabUsername: $('#qlabUsername'), qlabPassword: $('#qlabPassword'),
  qlabConnectButton: $('#qlabConnectButton'), qlabLoginStatus: $('#qlabLoginStatus'),
  qlabRemoteLoginHelp: $('#qlabRemoteLoginHelp'),
  toast: $('#toast'), toastIcon: $('#toastIcon'), toastTitle: $('#toastTitle'),
  toastDetail: $('#toastDetail'), toastCloseButton: $('#toastCloseButton')
};

let settings = emptySettings();
let availableShows = [];
let discoveredAvantis = [];
let discoveredQLab = [];
let availableWorkspaces = [];
let states = { avantis: 'not-configured', qlab: 'not-configured' };
let qlabAccessReady = new Set();
let busy = { avantisScan: false, shows: false, qlabScan: false, qlab: false, connectionCheck: false, backup: false };
let saveTimer;
let toastTimer;
let hasAutoScanned = false;
let connectionCheckPromise;

function normaliseRemote(stored) {
  return {
    host: String(stored?.host || ''),
    name: String(stored?.name || ''),
    port: Number(stored?.port || 22),
    username: String(stored?.username || ''),
    fingerprint: String(stored?.fingerprint || ''),
    local: Boolean(stored?.local)
  };
}

function normaliseSettings(stored) {
  const next = emptySettings();
  next.backupFolder = String(stored?.backupFolder || '');
  next.devices.avantis.endpoint = String(stored?.devices?.avantis?.endpoint || '');
  next.devices.avantis.selectedShows = Array.isArray(stored?.devices?.avantis?.selectedShows)
    ? stored.devices.avantis.selectedShows.map(String) : [];
  const storedQLabHost = String(stored?.devices?.qlab?.instance?.host || '');
  const storedQLabLocal = Boolean(stored?.devices?.qlab?.instance?.local);
  next.devices.qlab.instance = {
    host: storedQLabLocal && storedQLabHost ? '127.0.0.1' : storedQLabHost,
    address: String(stored?.devices?.qlab?.instance?.address || storedQLabHost),
    hostname: String(stored?.devices?.qlab?.instance?.hostname || ''),
    name: String(stored?.devices?.qlab?.instance?.name || ''),
    oscPort: Number(stored?.devices?.qlab?.instance?.oscPort || 53000),
    local: storedQLabLocal
  };
  next.devices.qlab.remote = normaliseRemote(stored?.devices?.qlab?.remote);
  next.devices.qlab.selectedWorkspaces = Array.isArray(stored?.devices?.qlab?.selectedWorkspaces)
    ? stored.devices.qlab.selectedWorkspaces.map(workspace => ({
      id: String(workspace?.id || ''), name: String(workspace?.name || ''),
      port: Number(workspace?.port || 53000), version: String(workspace?.version || '')
    })).filter(workspace => workspace.id) : [];
  return next;
}

function selectedShows() { return settings.devices.avantis.selectedShows; }
function selectedWorkspaces() { return settings.devices.qlab.selectedWorkspaces; }
function fileName(path) { return path.split(/[\\/]/).filter(Boolean).at(-1) || path; }

function showToast(title, detail = '', kind = 'success', duration = 5000) {
  clearTimeout(toastTimer);
  elements.toastTitle.textContent = title;
  elements.toastDetail.textContent = detail;
  elements.toast.classList.toggle('error', kind === 'error');
  elements.toastIcon.className = `fa-solid ${kind === 'error' ? 'fa-exclamation' : 'fa-check'}`;
  elements.toast.hidden = false;
  toastTimer = setTimeout(() => { elements.toast.hidden = true; }, duration);
}

function setInlineStatus(element, text, kind = '') {
  element.textContent = text;
  element.className = `inline-status${kind ? ` ${kind}` : ''}`;
}

function setProviderState(provider, state) {
  states[provider] = state;
  const labels = { 'not-configured': 'Set up', checking: 'Checking…', connected: 'Connected', offline: 'Needs attention' };
  const indicator = elements[`${provider}Indicator`];
  const indicatorStatus = elements[`${provider}IndicatorStatus`];
  const providerName = provider === 'qlab' ? 'QLab' : 'Avantis';
  indicator.dataset.state = state;
  indicatorStatus.textContent = labels[state];
  indicator.setAttribute('aria-label', `${providerName}: ${labels[state]}`);
  renderHomeSummary();
}

function configuredProviders() {
  const providers = [];
  if (settings.devices.avantis.endpoint && selectedShows().length) providers.push('Avantis');
  const qlabRemoteReady = settings.devices.qlab.instance.local || Boolean(settings.devices.qlab.remote.fingerprint);
  if (settings.devices.qlab.instance.host && selectedWorkspaces().length && qlabRemoteReady) providers.push('QLab');
  return providers;
}

function connectedProviders() {
  return configuredProviders().filter(provider => states[provider.toLocaleLowerCase()] === 'connected');
}

function renderHomeSummary() {
  const issue = configurationIssue();
  const connected = connectedProviders();
  const waitingForConnection = !issue && connected.length === 0;
  const needsLocation = issue?.target === 'location';
  elements.backupButtonLabel.textContent = needsLocation
    ? 'Choose backup location'
    : issue ? 'Set up backup' : waitingForConnection ? 'Check connections' : 'Back up';
  elements.backupButtonIcon.className = `fa-solid ${needsLocation ? 'fa-folder' : issue ? 'fa-arrow-right' : waitingForConnection ? 'fa-rotate' : 'fa-download'}`;
  elements.backupSummary.hidden = Boolean(issue) || waitingForConnection;
  if (!issue && connected.length) elements.backupSummary.textContent = `${connected.join(' · ')} · ${fileName(settings.backupFolder)}`;
}

function renderLocation() {
  const configured = Boolean(settings.backupFolder);
  elements.backupFolderDisplay.textContent = configured ? settings.backupFolder : 'Choose a folder';
  elements.backupFolderDisplay.title = settings.backupFolder;
  elements.backupFolderDisplay.classList.toggle('configured', configured);
}

function renderAvantisChoices() {
  const endpoint = settings.devices.avantis.endpoint;
  const choices = [...discoveredAvantis];
  if (endpoint && !choices.some(device => device.endpoint === endpoint)) choices.unshift({ endpoint, name: 'Saved Avantis' });
  elements.deviceChoices.replaceChildren();
  for (const device of choices) {
    elements.deviceChoices.append(createDeviceChoice({
      name: device.name || 'Avantis', detail: device.endpoint, selected: device.endpoint === endpoint,
      radioName: 'avantisDevice',
      onChange: () => selectAvantis(device.endpoint)
    }));
  }
}

function createDeviceChoice({ name, detail, selected, radioName, onChange }) {
  const label = document.createElement('label');
  label.className = `device-choice${selected ? ' selected' : ''}`;
  const input = document.createElement('input');
  input.type = 'radio'; input.name = radioName; input.checked = selected;
  input.addEventListener('change', onChange);
  const copy = document.createElement('span'); copy.className = 'device-choice-copy';
  const strong = document.createElement('strong'); strong.textContent = name;
  const small = document.createElement('span'); small.textContent = detail;
  copy.append(strong, small);
  label.append(input, copy);
  return label;
}

function renderShows() {
  const query = elements.showSearch.value.trim().toLocaleLowerCase();
  const selected = new Set(selectedShows().map(name => name.toLocaleLowerCase()));
  const visible = availableShows.filter(show => show.name.toLocaleLowerCase().includes(query));
  elements.showChoices.replaceChildren();
  elements.showCountText.textContent = availableShows.length ? `${selectedShows().length} selected` : '';
  elements.showToolbar.hidden = availableShows.length === 0;
  elements.selectAllShowsButton.textContent = availableShows.length && selectedShows().length === availableShows.length ? 'Clear all' : 'Select all';
  if (!busy.shows && visible.length === 0) {
    const empty = document.createElement('div'); empty.className = 'empty-choice';
    if (query) empty.textContent = 'No matching Shows';
    else {
      const message = document.createElement('span');
      message.textContent = states.avantis === 'offline' ? 'Check the Avantis, then refresh.' : 'Store a Show on the Avantis, then refresh.';
      const refresh = document.createElement('button'); refresh.type = 'button'; refresh.className = 'secondary-button compact';
      refresh.innerHTML = '<i class="fa-solid fa-rotate" aria-hidden="true"></i><span>Refresh</span>';
      refresh.addEventListener('click', () => refreshShows()); empty.append(message, refresh);
    }
    elements.showChoices.append(empty); return;
  }
  for (const show of visible) {
    const label = document.createElement('label'); label.className = 'show-option';
    const input = document.createElement('input'); input.type = 'checkbox'; input.checked = selected.has(show.name.toLocaleLowerCase());
    input.addEventListener('change', () => toggleShow(show.name, input.checked));
    const name = document.createElement('span'); name.textContent = show.name; label.append(input, name); elements.showChoices.append(label);
  }
}

function renderQLab() {
  const qlab = settings.devices.qlab;
  const choices = [...discoveredQLab];
  if (qlab.instance.host && !choices.some(instance => sameQLab(instance, qlab.instance))) choices.unshift(qlab.instance);
  elements.qlabInstanceChoices.replaceChildren();
  for (const instance of choices) {
    const selected = sameQLab(instance, qlab.instance);
    const projectNames = Array.isArray(instance.workspaceNames) && instance.workspaceNames.length
      ? instance.workspaceNames
      : selected ? selectedWorkspaces().map(workspace => workspace.name) : [];
    const hostDetails = [...new Set([
      instance.hostname,
      instance.address || instance.host,
      instance.local ? 'This Mac' : ''
    ].filter(Boolean))].join(' · ');
    elements.qlabInstanceChoices.append(createDeviceChoice({
      name: projectNames.length ? projectNames.join(', ') : instance.name || 'QLab',
      detail: hostDetails, selected,
      radioName: 'qlabInstance',
      onChange: () => selectQLab(instance)
    }));
  }
  elements.qlabLogin.hidden = !qlab.instance.host || qlab.instance.local;
  elements.qlabUsername.value = qlab.remote.username;
}

function renderSettings() {
  renderLocation();
  elements.manualAddress.value = settings.devices.avantis.endpoint;
  elements.showSettings.hidden = !settings.devices.avantis.endpoint;
  renderAvantisChoices(); renderShows(); renderQLab();
  renderHomeSummary();
}

function sameQLab(a, b) {
  if (!a?.host || !b?.host || Number(a.oscPort) !== Number(b.oscPort)) return false;
  return Boolean(a.local) && Boolean(b.local) ? true : a.host === b.host;
}

function saveSettingsSoon() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    saveTimer = undefined;
    try { await invoke('save_settings', { settings }); }
    catch (error) { showToast('Couldn’t save Settings', String(error), 'error'); }
  }, 250);
}

async function saveSettingsNow() {
  clearTimeout(saveTimer); saveTimer = undefined;
  await invoke('save_settings', { settings });
}

function guideTo(target) {
  const targets = {
    location: elements.chooseFolderButton, avantis: elements.discoverButton,
    shows: availableShows.length ? elements.showSearch : elements.refreshShowsButton,
    qlab: elements.qlabDiscoverButton,
    devices: elements.discoverButton
  };
  const element = targets[target]; if (!element) return;
  element.scrollIntoView({ behavior: 'smooth', block: 'center' }); element.focus({ preventScroll: true });
}

function showView(name, target) {
  const settingsVisible = name === 'settings';
  elements.homeView.hidden = settingsVisible; elements.settingsView.hidden = !settingsVisible;
  if (settingsVisible) {
    renderSettings();
    if (!hasAutoScanned) {
      hasAutoScanned = true;
      discoverAvantis(); discoverQLab();
    }
    if (target) requestAnimationFrame(() => guideTo(target));
  }
}

async function chooseFolder() {
  try {
    const selected = await invoke('choose_backup_folder'); if (!selected) return;
    settings.backupFolder = selected; renderLocation(); renderHomeSummary(); await saveSettingsNow();
    if (!configuredProviders().length) guideTo('devices');
  } catch (error) { showToast('Couldn’t choose that folder', String(error), 'error'); }
}

async function discoverAvantis() {
  if (busy.avantisScan) return;
  busy.avantisScan = true; elements.discoverButton.disabled = true; elements.discoverButtonLabel.textContent = 'Scanning';
  setInlineStatus(elements.discoveryStatus, 'Looking for consoles…');
  if (!settings.devices.avantis.endpoint) setProviderState('avantis', 'checking');
  try {
    const devices = await invoke('discover_avantis', { knownAddress: settings.devices.avantis.endpoint || null });
    discoveredAvantis = Array.isArray(devices) ? devices : []; renderAvantisChoices();
    if (!discoveredAvantis.length) {
      setInlineStatus(elements.discoveryStatus, 'No Avantis found. Check the network, then scan again.');
      if (!settings.devices.avantis.endpoint) setProviderState('avantis', 'not-configured');
    } else {
      setInlineStatus(elements.discoveryStatus, '');
      if (!settings.devices.avantis.endpoint && discoveredAvantis.length === 1) await selectAvantis(discoveredAvantis[0].endpoint);
    }
  } catch {
    setInlineStatus(elements.discoveryStatus, 'Scan failed. Try again or connect by IP.', 'error');
    if (!settings.devices.avantis.endpoint) setProviderState('avantis', 'not-configured');
  } finally {
    busy.avantisScan = false; elements.discoverButton.disabled = false; elements.discoverButtonLabel.textContent = 'Scan again';
  }
}

async function selectAvantis(endpoint) {
  const changed = settings.devices.avantis.endpoint !== endpoint;
  settings.devices.avantis.endpoint = endpoint;
  if (changed) settings.devices.avantis.selectedShows = [];
  elements.manualAddress.value = endpoint; renderSettings(); await saveSettingsNow(); await refreshShows();
}

async function connectManualAddress() {
  const endpoint = elements.manualAddress.value.trim();
  if (!endpoint) return showToast('Enter an IP address', '', 'error');
  await selectAvantis(endpoint);
}

async function refreshShows(options = {}) {
  const endpoint = settings.devices.avantis.endpoint; if (!endpoint || busy.shows) return;
  busy.shows = true; setProviderState('avantis', 'checking'); elements.refreshShowsButton.disabled = true;
  elements.showSettings.hidden = false; setInlineStatus(elements.showStatus, 'Loading Shows…'); renderShows();
  try {
    const shows = await invoke('load_avantis_shows', { consoleAddress: endpoint });
    availableShows = Array.isArray(shows) ? shows : [];
    const names = new Map(availableShows.map(show => [show.name.toLocaleLowerCase(), show.name]));
    settings.devices.avantis.selectedShows = selectedShows().map(name => names.get(name.toLocaleLowerCase())).filter(Boolean);
    setProviderState('avantis', 'connected'); setInlineStatus(elements.showStatus, ''); await saveSettingsNow();
  } catch {
    availableShows = []; setProviderState('avantis', 'offline'); setInlineStatus(elements.showStatus, '');
    if (!options.silent) showToast('Avantis unavailable', 'Check the console and network, then try again.', 'error');
  } finally {
    busy.shows = false; elements.refreshShowsButton.disabled = false; renderSettings();
  }
}

function toggleShow(name, checked) {
  const key = name.toLocaleLowerCase();
  const next = selectedShows().filter(show => show.toLocaleLowerCase() !== key); if (checked) next.push(name);
  settings.devices.avantis.selectedShows = next; renderShows(); renderHomeSummary(); saveSettingsSoon();
}

function toggleAllShows() {
  settings.devices.avantis.selectedShows = selectedShows().length === availableShows.length ? [] : availableShows.map(show => show.name);
  renderShows(); renderHomeSummary(); saveSettingsSoon();
}

async function discoverQLab() {
  if (busy.qlabScan) return;
  busy.qlabScan = true; elements.qlabDiscoverButton.disabled = true; elements.qlabDiscoverButtonLabel.textContent = 'Scanning';
  setInlineStatus(elements.qlabDiscoveryStatus, 'Looking for QLab…');
  if (!settings.devices.qlab.instance.host) setProviderState('qlab', 'checking');
  try {
    const instances = await invoke('discover_qlab'); discoveredQLab = Array.isArray(instances) ? instances : []; renderQLab();
    if (!discoveredQLab.length) {
      setInlineStatus(elements.qlabDiscoveryStatus, 'Open QLab and enable OSC access, then scan again.');
      if (!settings.devices.qlab.instance.host) setProviderState('qlab', 'not-configured');
    } else {
      setInlineStatus(elements.qlabDiscoveryStatus, '');
      if (!settings.devices.qlab.instance.host && discoveredQLab.length === 1) await selectQLab(discoveredQLab[0]);
    }
  } catch (error) {
    setInlineStatus(elements.qlabDiscoveryStatus, String(error), 'error');
    if (!settings.devices.qlab.instance.host) setProviderState('qlab', 'not-configured');
  } finally {
    busy.qlabScan = false; elements.qlabDiscoverButton.disabled = false; elements.qlabDiscoverButtonLabel.textContent = 'Scan again';
  }
}

async function selectQLab(instance) {
  const qlab = settings.devices.qlab;
  const same = sameQLab(instance, qlab.instance);
  qlab.instance = {
    host: instance.host, address: instance.address || instance.host, hostname: instance.hostname || '', name: instance.name || 'QLab',
    oscPort: Number(instance.oscPort || 53000), local: Boolean(instance.local)
  };
  qlab.remote = {
    host: instance.host, name: instance.hostname || instance.name || 'QLab Mac', port: 22,
    username: same ? qlab.remote.username : '',
    fingerprint: same ? qlab.remote.fingerprint : '', local: Boolean(instance.local)
  };
  if (!same) qlab.selectedWorkspaces = [];
  qlabAccessReady.clear(); setProviderState('qlab', 'checking'); renderQLab(); await saveSettingsNow();
  await refreshQLabWorkspaces();
  if (!instance.local && qlab.remote.username && !qlab.remote.fingerprint) await connectQLabRemote({ silent: true });
}

async function refreshQLabWorkspaces(options = {}) {
  const instance = settings.devices.qlab.instance; if (!instance.host || busy.qlab) return;
  busy.qlab = true; setProviderState('qlab', 'checking');
  setInlineStatus(elements.qlabWorkspaceStatus, 'Loading projects…');
  try {
    const workspaces = await invoke('load_qlab_workspaces', { input: { host: instance.host, oscPort: instance.oscPort } });
    availableWorkspaces = Array.isArray(workspaces) ? workspaces : [];
    settings.devices.qlab.selectedWorkspaces = [...availableWorkspaces];
    const discovered = discoveredQLab.find(item => sameQLab(item, instance));
    if (discovered) discovered.workspaceNames = availableWorkspaces.map(workspace => workspace.name);
    setInlineStatus(elements.qlabWorkspaceStatus, availableWorkspaces.length ? '' : 'Open a saved project in QLab, then scan again.');
    await validateSelectedQLabAccess({ silent: true }); await saveSettingsNow();
  } catch (error) {
    availableWorkspaces = []; setProviderState('qlab', 'offline'); setInlineStatus(elements.qlabWorkspaceStatus, String(error), 'error');
    if (!options.silent) showToast('QLab unavailable', String(error), 'error');
  } finally {
    busy.qlab = false; renderQLab(); updateQLabState();
  }
}

async function validateQLabAccess(workspace, passcode, options = {}) {
  try {
    await invoke('authorise_qlab_workspace', { input: { instance: settings.devices.qlab.instance, workspace, passcode } });
    qlabAccessReady.add(workspace.id); elements.qlabPasscodePanel.hidden = true; setInlineStatus(elements.qlabWorkspaceStatus, ''); return true;
  } catch (error) {
    qlabAccessReady.delete(workspace.id);
    if (String(error).toLocaleLowerCase().includes('passcode')) elements.qlabPasscodePanel.hidden = false;
    setInlineStatus(elements.qlabWorkspaceStatus, String(error), 'error');
    if (!options.silent) showToast('QLab needs attention', String(error), 'error', 8000);
    return false;
  }
}

async function validateSelectedQLabAccess(options = {}) {
  qlabAccessReady.clear();
  for (const workspace of selectedWorkspaces()) await validateQLabAccess(workspace, null, options);
}

async function unlockQLab() {
  const passcode = elements.qlabPasscode.value;
  if (!passcode) return showToast('Enter the workspace passcode', '', 'error');
  elements.qlabUnlockButton.disabled = true;
  let success = true;
  const locked = selectedWorkspaces().filter(workspace => !qlabAccessReady.has(workspace.id));
  for (const workspace of locked) success = await validateQLabAccess(workspace, passcode, { silent: true }) && success;
  elements.qlabUnlockButton.disabled = false;
  elements.qlabPasscode.value = '';
  if (success) { elements.qlabPasscodePanel.hidden = true; showToast('QLab unlocked'); }
  else if (qlabAccessReady.size) showToast('Some workspaces unlocked', 'Enter the passcode for the remaining workspace.');
  else showToast('That passcode didn’t work', '', 'error');
  updateQLabState();
}

async function connectQLabRemote(options = {}) {
  const remote = settings.devices.qlab.remote; if (!remote.host || remote.local) return;
  remote.username = elements.qlabUsername.value.trim() || remote.username;
  if (!remote.username) return showToast('Enter the Mac account name', '', 'error');
  elements.qlabConnectButton.disabled = true; setInlineStatus(elements.qlabLoginStatus, 'Connecting…');
  try {
    const result = await invoke('test_remote_access', { input: { remote, password: options.usePassword ? elements.qlabPassword.value : null } });
    remote.fingerprint = result.fingerprint; elements.qlabPassword.value = ''; setInlineStatus(elements.qlabLoginStatus, 'Connected', 'success');
    await saveSettingsNow(); updateQLabState();
  } catch (error) {
    remote.fingerprint = ''; setInlineStatus(elements.qlabLoginStatus, String(error), 'error'); setProviderState('qlab', 'offline');
    if (!options.silent) showToast('Couldn’t connect to QLab files', String(error), 'error', 8000);
  } finally { elements.qlabConnectButton.disabled = false; renderQLab(); }
}

function updateQLabState() {
  const qlab = settings.devices.qlab;
  if (!qlab.instance.host) return setProviderState('qlab', 'not-configured');
  const fileAccess = qlab.instance.local || Boolean(qlab.remote.fingerprint);
  const workspaceAccess = selectedWorkspaces().length && selectedWorkspaces().every(workspace => qlabAccessReady.has(workspace.id));
  setProviderState('qlab', fileAccess && workspaceAccess ? 'connected' : 'offline');
}

function workspaceIds(workspaces) {
  return workspaces.map(workspace => workspace.id).sort().join('\n');
}

async function checkAvantisConnection() {
  const endpoint = settings.devices.avantis.endpoint;
  if (!endpoint || busy.avantisScan || busy.shows) return;
  try {
    await invoke('test_avantis', { consoleAddress: endpoint });
    setProviderState('avantis', 'connected');
    setInlineStatus(elements.showStatus, '');
  } catch {
    setProviderState('avantis', 'offline');
    setInlineStatus(elements.showStatus, 'Check the console and network, then try again.', 'error');
  }
}

async function checkQLabConnection() {
  const instance = settings.devices.qlab.instance;
  if (!instance.host || busy.qlabScan || busy.qlab) return;
  try {
    const workspaces = await invoke('load_qlab_workspaces', {
      input: { host: instance.host, oscPort: instance.oscPort }
    });
    const openWorkspaces = Array.isArray(workspaces) ? workspaces : [];
    if (!openWorkspaces.length) {
      availableWorkspaces = [];
      qlabAccessReady.clear();
      setProviderState('qlab', 'offline');
      setInlineStatus(elements.qlabWorkspaceStatus, 'Open a saved project in QLab, then scan again.', 'error');
      return;
    }

    const changed = workspaceIds(openWorkspaces) !== workspaceIds(selectedWorkspaces());
    availableWorkspaces = openWorkspaces;
    settings.devices.qlab.selectedWorkspaces = [...openWorkspaces];
    const discovered = discoveredQLab.find(item => sameQLab(item, instance));
    if (discovered) discovered.workspaceNames = openWorkspaces.map(workspace => workspace.name);
    setInlineStatus(elements.qlabWorkspaceStatus, '');

    const needsAccessCheck = openWorkspaces.some(workspace => !qlabAccessReady.has(workspace.id));
    if (needsAccessCheck) await validateSelectedQLabAccess({ silent: true });
    updateQLabState();
    if (changed) saveSettingsSoon();
    renderQLab();
  } catch {
    availableWorkspaces = [];
    qlabAccessReady.clear();
    setProviderState('qlab', 'offline');
    setInlineStatus(elements.qlabWorkspaceStatus, 'QLab is no longer available.', 'error');
  }
}

async function checkConnections(options = {}) {
  if (busy.connectionCheck) return connectionCheckPromise;
  if (busy.backup) return;
  if (!options.force && document.hidden) return;
  busy.connectionCheck = true;
  connectionCheckPromise = Promise.all([checkAvantisConnection(), checkQLabConnection()]);
  try {
    await connectionCheckPromise;
  } finally {
    busy.connectionCheck = false;
    connectionCheckPromise = undefined;
    renderHomeSummary();
  }
}

function configurationIssue() {
  if (!settings.backupFolder) return { target: 'location' };
  if (configuredProviders().length) return null;
  if (settings.devices.avantis.endpoint && !selectedShows().length) return { target: 'shows' };
  if (settings.devices.qlab.instance.host) return { target: 'qlab' };
  return { target: 'devices' };
}

async function runBackup() {
  if (busy.backup) return;
  const issue = configurationIssue();
  if (issue?.target === 'location') return chooseFolder();
  if (issue) return showView('settings', issue.target);
  elements.backupButton.disabled = true;
  await checkConnections({ force: true });
  const connected = new Set(connectedProviders());
  if (!connected.size) {
    elements.backupButton.disabled = false;
    showToast('Nothing connected', 'Open QLab or check the Avantis connection, then try again.', 'error', 8000);
    return;
  }

  busy.backup = true; elements.progressPanel.hidden = false;
  const failures = []; let fileCount = 0;
  const tasks = [];
  if (connected.has('Avantis')) tasks.push({
    name: 'Avantis', detail: `${selectedShows().length} Show${selectedShows().length === 1 ? '' : 's'}`,
    run: () => invoke('backup_avantis_shows', { input: { consoleAddress: settings.devices.avantis.endpoint, showNames: selectedShows(), backupFolder: settings.backupFolder } })
  });
  if (connected.has('QLab')) tasks.push({
    name: 'QLab', detail: `${selectedWorkspaces().length} workspace${selectedWorkspaces().length === 1 ? '' : 's'}`,
    run: () => invoke('backup_qlab_workspaces', { input: { instance: settings.devices.qlab.instance, remote: settings.devices.qlab.remote, workspaces: selectedWorkspaces(), backupFolder: settings.backupFolder } })
  });
  try {
    await saveSettingsNow();
    for (const task of tasks) {
      elements.progressTitle.textContent = `Backing up ${task.name}`; elements.progressDetail.textContent = task.detail;
      try { const result = await task.run(); fileCount += result.files?.length || 1; }
      catch (error) { failures.push(`${task.name}: ${String(error)}`); }
    }
    elements.progressPanel.hidden = true;
    if (failures.length) showToast(fileCount ? 'Backup partly complete' : 'Backup didn’t finish', failures.join(' · '), 'error', 12000);
    else showToast('Backup complete', `${fileCount} ${fileCount === 1 ? 'file' : 'files'} saved to ${fileName(settings.backupFolder)}.`);
  } finally {
    busy.backup = false; elements.backupButton.disabled = false; elements.progressPanel.hidden = true;
  }
}

async function load() {
  try { settings = normaliseSettings(await invoke('load_settings')); }
  catch (error) { showToast('Couldn’t load Settings', String(error), 'error'); }
  setProviderState('avantis', settings.devices.avantis.endpoint ? 'checking' : 'not-configured');
  setProviderState('qlab', settings.devices.qlab.instance.host ? 'checking' : 'not-configured');
  renderSettings();
  if (settings.devices.avantis.endpoint) refreshShows({ silent: true });
  if (settings.devices.qlab.instance.host) {
    refreshQLabWorkspaces({ silent: true });
    if (!settings.devices.qlab.instance.local && settings.devices.qlab.remote.username) connectQLabRemote({ silent: true });
  }
  window.setInterval(() => checkConnections(), 5000);
}

elements.settingsButton.addEventListener('click', () => showView('settings'));
elements.avantisIndicator.addEventListener('click', () => showView('settings', 'avantis'));
elements.qlabIndicator.addEventListener('click', () => showView('settings', 'qlab'));
elements.settingsBackButton.addEventListener('click', () => showView('home'));
elements.chooseFolderButton.addEventListener('click', chooseFolder);
elements.discoverButton.addEventListener('click', discoverAvantis);
elements.manualConnectButton.addEventListener('click', connectManualAddress);
elements.manualAddress.addEventListener('keydown', event => { if (event.key === 'Enter') connectManualAddress(); });
elements.refreshShowsButton.addEventListener('click', () => refreshShows());
elements.showSearch.addEventListener('input', renderShows);
elements.selectAllShowsButton.addEventListener('click', toggleAllShows);
elements.qlabDiscoverButton.addEventListener('click', discoverQLab);
elements.qlabRemoteLoginHelp.addEventListener('click', () => invoke('open_remote_login_settings'));
elements.qlabUnlockButton.addEventListener('click', unlockQLab);
elements.qlabPasscode.addEventListener('keydown', event => { if (event.key === 'Enter') unlockQLab(); });
elements.qlabConnectButton.addEventListener('click', () => connectQLabRemote({ usePassword: true }));
elements.qlabPassword.addEventListener('keydown', event => { if (event.key === 'Enter') connectQLabRemote({ usePassword: true }); });
elements.backupButton.addEventListener('click', runBackup);
elements.toastCloseButton.addEventListener('click', () => { elements.toast.hidden = true; });
document.addEventListener('keydown', event => { if (event.key === 'Escape' && !elements.settingsView.hidden) showView('home'); });
document.addEventListener('visibilitychange', () => { if (!document.hidden) checkConnections({ force: true }); });

load();
