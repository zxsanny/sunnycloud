import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import RouterHarness from './fixtures/RouterHarness.svelte';
import { navigate, route, startRouter } from './router.svelte';

afterEach(() => navigate('/', { replace: true }));

describe('the router', () => {
  it('shows the page of the current path', () => {
    // Arrange
    navigate('/login', { replace: true });

    // Act
    render(RouterHarness);

    // Assert
    expect(screen.getByText('login page')).toBeInTheDocument();
  });

  it('shows the not-found page for an unknown path', () => {
    // Arrange
    navigate('/nowhere', { replace: true });

    // Act
    render(RouterHarness);

    // Assert
    expect(screen.getByText('page not found')).toBeInTheDocument();
  });

  it('switches page when navigate is called', async () => {
    // Arrange
    render(RouterHarness);

    // Act
    navigate('/login');

    // Assert
    expect(await screen.findByText('login page')).toBeInTheDocument();
  });

  it('follows a same-origin link without a page load', async () => {
    // Arrange
    const stop = startRouter();
    render(RouterHarness);
    const user = userEvent.setup();

    // Act
    await user.click(screen.getByRole('link', { name: 'to login' }));

    // Assert
    expect(await screen.findByText('login page')).toBeInTheDocument();
    expect(route.path).toBe('/login');
    stop();
  });

  it('leaves links to the API to the browser', async () => {
    // Arrange
    const stop = startRouter();
    // Registered after the router, so it only stops jsdom from attempting a real page load.
    document.addEventListener('click', (event) => event.preventDefault());
    render(RouterHarness);
    const user = userEvent.setup();

    // Act
    await user.click(screen.getByRole('link', { name: 'download' }));

    // Assert
    expect(route.path).toBe('/');
    stop();
  });

  it('keeps the query and the fragment readable', () => {
    // Act
    navigate('/invite?return=%2Ffiles#sc_inv_abc', { replace: true });

    // Assert
    expect(route.query.get('return')).toBe('/files');
    expect(route.hash).toBe('#sc_inv_abc');
    expect(route.relative).toBe('/invite?return=%2Ffiles');
  });
});
