import { mount } from 'svelte';
import App from './App.svelte';
import './daisy.css';
import './tokens.css';
mount(App, { target: document.getElementById('app')! });
