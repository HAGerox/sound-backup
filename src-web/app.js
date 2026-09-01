const nativeInvoke = window.__TAURI__?.core?.invoke;

const emptySettings = () => ({
  backupFolder: '',
  devices: {
    avantis: {
      endpoint: '',
      selectedShows: []
    }
  }
});

async function invoke(command, arguments_) {
  if (nativeInvoke) return nativeInvoke(command, arguments_);
  if (command === 'load_settings') return emptySettings();
  if (command === 'save_settings') return;
  if (command === 'discover_avantis' || command === 'load_avantis_shows') return [];
  throw new Error('This action is available in the Stage Backup desktop app.');
}

const elements = {
  homeView: document.querySelector('#homeView'),
  settingsView: document.querySelector('#settingsView'),
  settingsButton: document.querySelector('#settingsButton'),
  settingsBackButton: document.querySelector('#settingsBackButton'),
  backupButton: document.querySelector('#backupButton'),
  backupButtonIcon: document.querySelector('#backupButtonIcon'),
  backupButtonLabel: document.querySelector('#backupButtonLabel'),
  backupSummary: document.querySelector('#backupSummary'),
  avantisIndicator: document.querySelector('#avantisIndicator'),
  avantisIndicatorStatus: document.querySelector('#avantisIndicatorStatus'),
  progressPanel: document.querySelector('#progressPanel'),
  progressTitle: document.querySelector('#progressTitle'),
  progressDetail: document.querySelector('#progressDetail'),
  backupFolderDisplay: document.querySelector('#backupFolderDisplay'),
  chooseFolderButton: document.querySelector('#chooseFolderButton'),
  avantisCardSubtitle: document.querySelector('#avantisCardSubtitle'),
  avantisStatusBadge: document.querySelector('#avantisStatusBadge'),
  discoverButton: document.querySelector('#discoverButton'),
  discoverButtonLabel: document.querySelector('#discoverButtonLabel'),
  discoveryStatus: document.querySelector('#discoveryStatus'),
  deviceChoices: document.querySelector('#deviceChoices'),
  manualAddress: document.querySelector('#manualAddress'),
  manualConnectButton: document.querySelector('#manualConnectButton'),
  showSettings: document.querySelector('#showSettings'),
  refreshShowsButton: document.querySelector('#refreshShowsButton'),
  showToolbar: document.querySelector('#showToolbar'),
  showSearch: document.querySelector('#showSearch'),
  selectAllShowsButton: document.querySelector('#selectAllShowsButton'),
  showStatus: document.querySelector('#showStatus'),
  showChoices: document.querySelector('#showChoices'),
  showCountText: document.querySelector('#showCountText'),
  toast: document.querySelector('#toast'),
  toastIcon: document.querySelector('#toastIcon'),
  toastTitle: document.querySelector('#toastTitle'),
  toastDetail: document.querySelector('#toastDetail'),
  toastCloseButton: document.querySelector('#toastCloseButton')
};

let settings = emptySettings();
let availableShows = [];
let discoveredDevices = [];
let connectionState = 'not-configured';
let discoveryBusy = false;
let showsBusy = false;
let backupBusy = false;
let saveTimer;
let toastTimer;
let hasAutoScanned = false;

function normaliseSettings(stored) {
  const next = emptySettings();
  next.backupFolder = String(stored?.backupFolder || '');
  next.devices.avantis.endpoint = String(stored?.devices?.avantis?.endpoint || '');
  next.devices.avantis.selectedShows = Array.isArray(stored?.devices?.avantis?.selectedShows)
    ? stored.devices.avantis.selectedShows.map(String)
    : [];
  return next;
}

function selectedShows() {
  return settings.devices.avantis.selectedShows;
}

function fileName(path) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) || path;
}

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

function setConnectionState(state) {
  connectionState = state;
  const labels = {
    'not-configured': 'Set up',
    checking: 'Checking…',
    connected: 'Connected',
    offline: 'Offline'
  };
  elements.avantisIndicator.dataset.state = state;
  elements.avantisIndicatorStatus.textContent = labels[state];
  elements.avantisIndicator.setAttribute(
    'aria-label',
    state === 'connected' ? 'Avantis connected' : state === 'offline' ? 'Reconnect Avantis' : 'Set up Avantis'
  );
  elements.avantisStatusBadge.dataset.state = state;
  elements.avantisStatusBadge.textContent = labels[state];
  elements.avantisStatusBadge.hidden = state === 'not-configured';
  renderHomeSummary();
}

function renderHomeSummary() {
  const showCount = selectedShows().length;
  const issue = configurationIssue();
  elements.backupButtonLabel.textContent = issue ? 'Set up backup' : 'Back up';
  elements.backupButtonIcon.className = `fa-solid ${issue ? 'fa-arrow-right' : 'fa-download'}`;
  elements.backupSummary.hidden = Boolean(issue);
  if (issue) {
    elements.backupSummary.textContent = '';
    return;
  }
  const showLabel = `${showCount} Show${showCount === 1 ? '' : 's'}`;
  elements.backupSummary.textContent = `${showLabel} · ${fileName(settings.backupFolder)}`;
}

function renderLocation() {
  const configured = Boolean(settings.backupFolder);
  elements.backupFolderDisplay.textContent = configured ? settings.backupFolder : 'Choose a folder';
  elements.backupFolderDisplay.title = settings.backupFolder;
  elements.backupFolderDisplay.classList.toggle('configured', configured);
}

function renderDeviceChoices() {
  const endpoint = settings.devices.avantis.endpoint;
  const choices = [...discoveredDevices];
  if (endpoint && !choices.some(device => device.endpoint === endpoint)) {
    choices.unshift({ endpoint, name: 'Saved Avantis' });
  }
  elements.deviceChoices.replaceChildren();
  if (choices.length === 0) return;

  for (const device of choices) {
    const label = document.createElement('label');
    label.className = `device-choice${device.endpoint === endpoint ? ' selected' : ''}`;
    const input = document.createElement('input');
    input.type = 'radio';
    input.name = 'avantisDevice';
    input.checked = device.endpoint === endpoint;
    input.addEventListener('change', () => selectDevice(device.endpoint));
    const copy = document.createElement('span');
    copy.className = 'device-choice-copy';
    const name = document.createElement('strong');
    name.textContent = device.name || 'Avantis';
    const address = document.createElement('span');
    address.textContent = device.endpoint;
    copy.append(name, address);
    const state = document.createElement('span');
    state.className = 'device-choice-state';
    state.textContent = device.endpoint === endpoint && connectionState === 'connected' ? 'Connected' : 'Available';
    label.append(input, copy, state);
    elements.deviceChoices.append(label);
  }
}

function renderShows() {
  const query = elements.showSearch.value.trim().toLocaleLowerCase();
  const selected = new Set(selectedShows().map(name => name.toLocaleLowerCase()));
  const visible = availableShows.filter(show => show.name.toLocaleLowerCase().includes(query));
  elements.showChoices.replaceChildren();
  elements.showCountText.textContent = availableShows.length
    ? selectedShows().length > 0 ? `${selectedShows().length} selected` : 'Choose Shows'
    : '';
  elements.showToolbar.hidden = availableShows.length === 0;
  elements.selectAllShowsButton.textContent = availableShows.length > 0 && selectedShows().length === availableShows.length
    ? 'Clear all'
    : 'Select all';

  if (!showsBusy && visible.length === 0) {
    const empty = document.createElement('div');
    empty.className = 'empty-choice';
    if (query) {
      empty.textContent = 'No matching Shows';
    } else {
      const message = document.createElement('span');
      message.textContent = connectionState === 'offline'
        ? 'Check the Avantis, then refresh.'
        : 'Store a Show on the Avantis, then refresh.';
      const refresh = document.createElement('button');
      refresh.type = 'button';
      refresh.className = 'secondary-button compact';
      refresh.innerHTML = '<i class="fa-solid fa-rotate" aria-hidden="true"></i><span>Refresh</span>';
      refresh.addEventListener('click', () => refreshShows());
      empty.append(message, refresh);
    }
    elements.showChoices.append(empty);
    return;
  }

  for (const show of visible) {
    const label = document.createElement('label');
    label.className = 'show-option';
    const input = document.createElement('input');
    input.type = 'checkbox';
    input.checked = selected.has(show.name.toLocaleLowerCase());
    input.addEventListener('change', () => toggleShow(show.name, input.checked));
    const name = document.createElement('span');
    name.textContent = show.name;
    label.append(input, name);
    elements.showChoices.append(label);
  }
}

function renderSettings() {
  renderLocation();
  const endpoint = settings.devices.avantis.endpoint;
  elements.manualAddress.value = endpoint;
  elements.avantisCardSubtitle.textContent = endpoint || 'Choose a console';
  elements.showSettings.hidden = !endpoint;
  renderDeviceChoices();
  renderShows();
}

function saveSettingsSoon() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(async () => {
    saveTimer = undefined;
    try {
      await invoke('save_settings', { settings });
    } catch (error) {
      showToast('Couldn’t save Settings', String(error), 'error');
    }
  }, 250);
}

async function saveSettingsNow() {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  await invoke('save_settings', { settings });
}

function guideTo(target) {
  const targets = {
    location: elements.chooseFolderButton,
    avantis: elements.discoverButton,
    shows: availableShows.length ? elements.showSearch : elements.refreshShowsButton
  };
  const element = targets[target];
  if (!element) return;
  element.scrollIntoView({ behavior: 'smooth', block: 'center' });
  element.focus({ preventScroll: true });
}

function showView(name, target) {
  const settingsVisible = name === 'settings';
  elements.homeView.hidden = settingsVisible;
  elements.settingsView.hidden = !settingsVisible;
  if (settingsVisible) {
    renderSettings();
    if (!hasAutoScanned) {
      hasAutoScanned = true;
      discoverAvantis();
    }
    if (target) requestAnimationFrame(() => guideTo(target));
  }
}

async function chooseFolder() {
  try {
    const selected = await invoke('choose_backup_folder');
    if (!selected) return;
    settings.backupFolder = selected;
    renderLocation();
    renderHomeSummary();
    await saveSettingsNow();
    if (!settings.devices.avantis.endpoint) guideTo('avantis');
  } catch (error) {
    showToast('Couldn’t choose that folder', String(error), 'error');
  }
}

async function discoverAvantis() {
  if (discoveryBusy) return;
  discoveryBusy = true;
  elements.discoverButton.disabled = true;
  elements.discoverButtonLabel.textContent = 'Scanning';
  setInlineStatus(elements.discoveryStatus, 'Looking for consoles…');
  if (!settings.devices.avantis.endpoint) setConnectionState('checking');
  try {
    const devices = await invoke('discover_avantis', {
      knownAddress: settings.devices.avantis.endpoint || null
    });
    discoveredDevices = Array.isArray(devices) ? devices : [];
    renderDeviceChoices();
    if (discoveredDevices.length === 0) {
      setInlineStatus(elements.discoveryStatus, 'No Avantis found.');
      if (!settings.devices.avantis.endpoint) setConnectionState('not-configured');
    } else {
      setInlineStatus(elements.discoveryStatus, '');
      if (!settings.devices.avantis.endpoint && discoveredDevices.length === 1) {
        await selectDevice(discoveredDevices[0].endpoint);
      }
    }
  } catch {
    setInlineStatus(elements.discoveryStatus, 'Scan failed. Try again or connect by IP.', 'error');
    if (!settings.devices.avantis.endpoint) setConnectionState('not-configured');
  } finally {
    discoveryBusy = false;
    elements.discoverButton.disabled = false;
    elements.discoverButtonLabel.textContent = 'Scan again';
  }
}

async function selectDevice(endpoint) {
  const changed = settings.devices.avantis.endpoint !== endpoint;
  settings.devices.avantis.endpoint = endpoint;
  if (changed) settings.devices.avantis.selectedShows = [];
  elements.manualAddress.value = endpoint;
  renderSettings();
  renderHomeSummary();
  await saveSettingsNow();
  await refreshShows();
}

async function connectManualAddress() {
  const endpoint = elements.manualAddress.value.trim();
  if (!endpoint) {
    showToast('Enter an IP address', '', 'error');
    return;
  }
  await selectDevice(endpoint);
}

async function refreshShows(options = {}) {
  const endpoint = settings.devices.avantis.endpoint;
  if (!endpoint || showsBusy) return;
  showsBusy = true;
  setConnectionState('checking');
  elements.refreshShowsButton.disabled = true;
  elements.showSettings.hidden = false;
  setInlineStatus(elements.showStatus, 'Loading Shows…');
  renderShows();
  try {
    const shows = await invoke('load_avantis_shows', { consoleAddress: endpoint });
    availableShows = Array.isArray(shows) ? shows : [];
    const availableNames = new Map(availableShows.map(show => [show.name.toLocaleLowerCase(), show.name]));
    settings.devices.avantis.selectedShows = selectedShows()
      .map(name => availableNames.get(name.toLocaleLowerCase()))
      .filter(Boolean);
    setConnectionState('connected');
    setInlineStatus(elements.showStatus, '');
    await saveSettingsNow();
  } catch {
    availableShows = [];
    setConnectionState('offline');
    setInlineStatus(elements.showStatus, '');
    if (!options.silent) showToast('Avantis unavailable', 'Check the console and network, then try again.', 'error');
  } finally {
    showsBusy = false;
    elements.refreshShowsButton.disabled = false;
    renderSettings();
    renderHomeSummary();
  }
}

function toggleShow(name, checked) {
  const normalised = name.toLocaleLowerCase();
  const next = selectedShows().filter(show => show.toLocaleLowerCase() !== normalised);
  if (checked) next.push(name);
  settings.devices.avantis.selectedShows = next;
  renderShows();
  renderHomeSummary();
  saveSettingsSoon();
}

function toggleAllShows() {
  settings.devices.avantis.selectedShows = selectedShows().length === availableShows.length
    ? []
    : availableShows.map(show => show.name);
  renderShows();
  renderHomeSummary();
  saveSettingsSoon();
}

function configurationIssue() {
  if (!settings.backupFolder) return { target: 'location' };
  if (!settings.devices.avantis.endpoint) return { target: 'avantis' };
  if (selectedShows().length === 0) return { target: 'shows' };
  return null;
}

async function runBackup() {
  if (backupBusy) return;
  const issue = configurationIssue();
  if (issue) {
    showView('settings', issue.target);
    return;
  }

  backupBusy = true;
  elements.backupButton.disabled = true;
  elements.progressPanel.hidden = false;
  elements.progressTitle.textContent = 'Backing up Avantis';
  elements.progressDetail.textContent = `Saving ${selectedShows().length} Show${selectedShows().length === 1 ? '' : 's'}…`;
  try {
    await saveSettingsNow();
    const result = await invoke('backup_avantis_shows', {
      input: {
        consoleAddress: settings.devices.avantis.endpoint,
        showNames: selectedShows(),
        backupFolder: settings.backupFolder
      }
    });
    const count = result.files?.length || selectedShows().length;
    elements.progressPanel.hidden = true;
    showToast('Backup complete', `${count} Show${count === 1 ? '' : 's'} saved to ${fileName(settings.backupFolder)}.`);
  } catch (error) {
    elements.progressPanel.hidden = true;
    showToast('Backup didn’t finish', String(error), 'error', 8000);
  } finally {
    backupBusy = false;
    elements.backupButton.disabled = false;
  }
}

async function load() {
  try {
    settings = normaliseSettings(await invoke('load_settings'));
  } catch (error) {
    showToast('Couldn’t load Settings', String(error), 'error');
  }
  setConnectionState(settings.devices.avantis.endpoint ? 'checking' : 'not-configured');
  renderSettings();
  renderHomeSummary();
  if (settings.devices.avantis.endpoint) refreshShows({ silent: true });
}

elements.settingsButton.addEventListener('click', () => showView('settings'));
elements.avantisIndicator.addEventListener('click', () => showView('settings', 'avantis'));
elements.settingsBackButton.addEventListener('click', () => showView('home'));
elements.chooseFolderButton.addEventListener('click', chooseFolder);
elements.discoverButton.addEventListener('click', discoverAvantis);
elements.manualConnectButton.addEventListener('click', connectManualAddress);
elements.manualAddress.addEventListener('keydown', event => {
  if (event.key === 'Enter') connectManualAddress();
});
elements.refreshShowsButton.addEventListener('click', () => refreshShows());
elements.showSearch.addEventListener('input', renderShows);
elements.selectAllShowsButton.addEventListener('click', toggleAllShows);
elements.backupButton.addEventListener('click', runBackup);
elements.toastCloseButton.addEventListener('click', () => { elements.toast.hidden = true; });
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && !elements.settingsView.hidden) showView('home');
});

load();
