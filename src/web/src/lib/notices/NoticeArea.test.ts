import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import NoticeArea from './NoticeArea.svelte';
import { notices } from './notices.svelte';

beforeEach(() => {
  notices.items = [];
});
afterEach(() => vi.useRealTimers());

describe('the notice area', () => {
  it('renders nothing while there is no notice', () => {
    // Act
    const { container } = render(NoticeArea);

    // Assert
    expect(container.querySelector('[role]')).toBeNull();
  });

  it('shows a server error with its reference in mono and a working copy action', async () => {
    // Arrange
    const user = userEvent.setup();
    notices.show({
      key: 'server',
      variant: 'danger',
      title: 'Something went wrong on the server.',
      text: 'Send this reference to your administrator:',
      requestId: 'r-9',
    });
    render(NoticeArea);

    // Act
    await user.click(screen.getByRole('button', { name: 'Copy reference' }));

    // Assert
    expect(screen.getByRole('alert')).toHaveTextContent('Something went wrong on the server.');
    expect(screen.getByText('r-9')).toHaveClass('font-code');
    expect(await navigator.clipboard.readText()).toBe('r-9');
  });

  it('counts down the seconds of a rate limit and removes the notice at zero', async () => {
    // Arrange
    vi.useFakeTimers({ now: 0, toFake: ['setInterval', 'clearInterval', 'Date'] });
    notices.show({
      key: 'rate',
      variant: 'warning',
      title: 'Too many requests.',
      text: '',
      retryAt: 3000,
    });
    render(NoticeArea);
    expect(screen.getByRole('alert')).toHaveTextContent('Try again in 3 seconds.');

    // Act
    await vi.advanceTimersByTimeAsync(1000);

    // Assert
    expect(screen.getByRole('alert')).toHaveTextContent('Try again in 2 seconds.');

    // Act
    await vi.advanceTimersByTimeAsync(2000);

    // Assert
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('lets the user dismiss a dismissible notice', async () => {
    // Arrange
    const user = userEvent.setup();
    notices.show({
      key: 'n',
      variant: 'info',
      title: 'Your session ended.',
      text: 'Sign in again.',
      dismissible: true,
    });
    render(NoticeArea);

    // Act
    await user.click(screen.getByRole('button', { name: 'Dismiss' }));

    // Assert
    expect(screen.queryByText('Your session ended.')).toBeNull();
  });
});
