/// <reference types="vite/client" />
import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

const target = document.getElementById('app');
if (!target) {
  throw new Error('Darkhold could not find its mount point (#app).');
}

export default mount(App, { target });

// The worker is registered by App.svelte once the shell mounts, so install and update can be
// offered to a reader. See src/lib/pwa.ts. Production only: a development worker would cache the
// modules being edited and make every change appear not to take effect.
