// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('alcoveShell', Object.freeze({
  command(command, visible) {
    const message = ['toolbar-visibility', 'menu-open'].includes(command)
      ? { command, visible }
      : { command };
    return ipcRenderer.invoke('alcove:shell-command', message);
  },
  onState(callback) {
    if (typeof callback !== 'function') return;
    ipcRenderer.on('alcove:shell-state', (_event, state) => callback(state));
    ipcRenderer.on('alcove:shell-reveal', () => callback({ reveal: true }));
  },
}));
