import { mount } from 'svelte';
import './styles/app.css';
import { App, handleApiError } from './app';
import { api } from '$lib/api';
import { startRouter } from '$lib/router';
import { theme } from '$lib/theme';

const target = document.getElementById('app');
if (target === null) {
  throw new Error('index.html has no #app element to mount the app into');
}

theme.init();
api.setErrorHandler(handleApiError);
startRouter();
mount(App, { target });
