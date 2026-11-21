import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { json, problem, stubServer, type Reply } from '../../tests/support/http';
import { App } from '../app';
import { api } from '$lib/api';
import { navigate } from '$lib/router';
import { session } from '$lib/session';
import { transfer } from '$lib/transfer';

const TOKEN = 'sc_shr_Q7mVb4LpT8wZr1NcY6dHs3EjA5uG0oRkWiPqXe9fK2x';
const SHARE = `/api/v1/public-shares/${TOKEN}`;
const NOW = '2026-11-21T10:00:00.000Z';

const available = (over: Record<string, unknown> = {}) => ({
  name: 'beach-sunset.jpg',
  size: 4_100_000,
  media_type: 'image/jpeg',
  expires_at: '2026-11-30T12:00:00Z',
  ...over,
});

function open(path = `/s/${TOKEN}`) {
  navigate(path, { replace: true });
  return render(App);
}

const pageText = () => (screen.getByRole('main') as HTMLElement).textContent ?? '';

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date(NOW));
  session.status = 'idle';
  session.account = null;
  api.setErrorHandler(() => {});
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  transfer.jobs = [];
  history.replaceState(null, '', '/');
});

describe('the public page: previews by type (AC-6)', () => {
  it('shows a JPEG inline with its name, size, type and expiry, without the app shell or a session call', async () => {
    // Arrange
    const server = stubServer({ [`GET ${SHARE}`]: json(available()) });

    // Act
    open();

    // Assert
    expect(await screen.findByRole('heading', { name: 'beach-sunset.jpg' })).toBeInTheDocument();
    expect(screen.getByText('4.1 MB · JPEG image · Available until 30 Nov 2026')).toBeVisible();
    expect(screen.getByRole('img', { name: 'beach-sunset.jpg' })).toHaveAttribute(
      'src',
      `${SHARE}/content`,
    );
    expect(screen.getByRole('button', { name: 'Download' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Sign out' })).not.toBeInTheDocument();
    expect(server.calls.map((call) => call.url.pathname)).toEqual([SHARE]);
  });

  it('plays an MP4 in a native player and shows no image', async () => {
    // Arrange
    stubServer({
      [`GET ${SHARE}`]: json(available({ name: 'kayak.mp4', media_type: 'video/mp4' })),
    });

    // Act
    const { container } = open();

    // Assert
    await screen.findByRole('heading', { name: 'kayak.mp4' });
    const video = container.querySelector('video') as HTMLVideoElement;
    expect(video).toHaveAttribute('src', `${SHARE}/content`);
    expect(video).toHaveAttribute('controls');
    expect(screen.queryByRole('img')).not.toBeInTheDocument();
    expect(screen.getByText(/MP4 video/)).toBeVisible();
  });

  it('offers a PDF as a download only: no image, video, frame or object', async () => {
    // Arrange
    stubServer({
      [`GET ${SHARE}`]: json(
        available({ name: 'itinerary.pdf', media_type: 'application/pdf', size: 1_200_000 }),
      ),
    });

    // Act
    const { container } = open();

    // Assert
    await screen.findByRole('heading', { name: 'itinerary.pdf' });
    expect(
      screen.getByText(
        "Preview isn't available for this file type. Download it to open it on your device.",
      ),
    ).toBeVisible();
    expect(screen.getByText(/PDF document/)).toBeVisible();
    expect(container.querySelector('img, video, iframe, object, embed')).toBeNull();
    expect(screen.getByRole('button', { name: 'Download' })).toBeEnabled();
  });

  it('shows hostile markup in a name as text and never as a preview (AC-11 names)', async () => {
    // Arrange
    const name = '<img src=x onerror=alert(1)>.html';
    stubServer({ [`GET ${SHARE}`]: json(available({ name, media_type: 'text/html' })) });

    // Act
    const { container } = open();

    // Assert
    expect(await screen.findByRole('heading', { name })).toBeInTheDocument();
    expect(container.querySelector('img, iframe, object, embed, script')).toBeNull();
  });

  it('downloads through the transfer service with the token it kept in memory, and says how it ended', async () => {
    // Arrange
    stubServer({
      [`GET ${SHARE}`]: json(available({ name: 'itinerary.pdf', media_type: 'application/pdf' })),
    });
    const download = vi
      .spyOn(transfer, 'download')
      .mockResolvedValue({ kind: 'SavedUnverified', reason: 'NoServerHash', serverHash: null });
    const user = userEvent.setup();
    open();
    await screen.findByRole('heading', { name: 'itinerary.pdf' });
    await waitFor(() => expect(location.pathname).toBe('/s'));

    // Act
    await user.click(screen.getByRole('button', { name: 'Download' }));

    // Assert
    expect(download).toHaveBeenCalledWith({ shareToken: TOKEN });
    expect(
      await screen.findByText(
        'Saved by the browser without a check (the server gives no file hash for this link).',
      ),
    ).toBeVisible();
  });
});

describe('the public page: one page for every failure (AC-7)', () => {
  async function failurePage(reply: Reply | null, path = `/s/${TOKEN}`) {
    stubServer(reply === null ? {} : { [`GET ${SHARE}`]: reply });
    const { unmount } = open(path);
    await screen.findByRole('heading', { name: "This link isn't available" });
    const snapshot = {
      html: (screen.getByRole('main') as HTMLElement).innerHTML,
      text: pageText(),
    };
    unmount();
    return snapshot;
  }

  it('renders byte-identical page text for an expired, a revoked and a nonexistent link', async () => {
    // Arrange: the server's one 404 answer; only the request id differs between them.
    const answers = ['expired', 'revoked', 'unknown'].map((cause, at) =>
      problem(404, 'not_found', { request_id: `0190aaaa-0000-7000-8000-00000000000${at}`, cause }),
    );

    // Act
    const pages = [];
    for (const answer of answers) pages.push(await failurePage(answer));

    // Assert
    expect(pages[0]?.text).toContain("This link isn't available");
    expect(pages[1]).toEqual(pages[0]);
    expect(pages[2]).toEqual(pages[0]);
  });

  it('shows the same page for a server fault, a dropped connection and an address without a token', async () => {
    // Arrange
    const expected = await failurePage(problem(404, 'not_found'));

    // Act
    const fault = await failurePage(problem(500, 'internal'));
    const dropped = await failurePage(() => {
      throw new TypeError('Failed to fetch');
    });
    const bare = await failurePage(null, '/s');

    // Assert
    expect(fault).toEqual(expected);
    expect(dropped).toEqual(expected);
    expect(bare).toEqual(expected);
  });

  it('sends no request when the address carries no token', async () => {
    // Arrange
    const server = stubServer({});

    // Act
    open('/s');
    await screen.findByRole('heading', { name: "This link isn't available" });

    // Assert
    expect(server.calls).toEqual([]);
  });
});

describe('the public page: a password link (AC-8, AC-11)', () => {
  // This stub models a server whose metadata read honours the grant; the real one does not yet (see the report).
  function protectedLink() {
    let unlocked = false;
    const server = stubServer({
      [`GET ${SHARE}`]: () =>
        unlocked
          ? json(
              available({
                name: 'secret-plan.pdf',
                size: 5_000_000,
                media_type: 'application/pdf',
              }),
            )
          : problem(401, 'password_required'),
      [`POST ${SHARE}/password-verifications`]: () => {
        const body = server.calls.at(-1)?.body as { password: string };
        if (body.password !== 'right-one') return problem(401, 'password_required');
        unlocked = true;
        return new Response(null, { status: 204 });
      },
    });
    return server;
  }
  const hidden = [
    'secret-plan',
    '5.0 MB',
    'PDF document',
    'application/pdf',
    'Available until',
    '30 Nov',
  ];

  it('shows nothing about the file before unlock, only the wrong-password line after a wrong one, then the file after the right one', async () => {
    // Arrange
    const server = protectedLink();
    const user = userEvent.setup();
    open();
    await screen.findByRole('heading', { name: 'This link is protected' });
    const before = document.body.textContent ?? '';

    // Act
    await user.type(screen.getByLabelText('Password'), 'wrong-one');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));
    const after = await screen.findByText("That password isn't right. Check it and try again.");

    // Assert (before unlock)
    for (const text of hidden) {
      expect(before).not.toContain(text);
      expect(document.body.textContent).not.toContain(text);
    }
    expect(after).toBeVisible();

    // Act (the right password)
    await user.clear(screen.getByLabelText('Password'));
    await user.type(screen.getByLabelText('Password'), 'right-one');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));

    // Assert
    expect(await screen.findByRole('heading', { name: 'secret-plan.pdf' })).toBeVisible();
    expect(screen.getByText(/5\.0 MB · PDF document · Available until 30 Nov 2026/)).toBeVisible();
    const posts = server.calls.filter((call) => call.method === 'POST');
    expect(posts.map((call) => call.body)).toEqual([
      { password: 'wrong-one' },
      { password: 'right-one' },
    ]);
  });

  it('says so when the password was accepted but the server still hides the file, and shows no file data', async () => {
    // Arrange
    stubServer({
      [`GET ${SHARE}`]: problem(401, 'password_required'),
      [`POST ${SHARE}/password-verifications`]: new Response(null, { status: 204 }),
    });
    const user = userEvent.setup();
    open();
    await screen.findByRole('heading', { name: 'This link is protected' });

    // Act
    await user.type(screen.getByLabelText('Password'), 'right-one');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));

    // Assert
    expect(
      await screen.findByText(
        'The password was accepted, but the server did not show the file. Try again later.',
      ),
    ).toBeVisible();
    expect(screen.queryByRole('img')).not.toBeInTheDocument();
  });

  it('keeps the share password in no browser storage and in no console line', async () => {
    // Arrange
    protectedLink();
    const lines = ['log', 'info', 'warn', 'error', 'debug'].map((level) =>
      vi.spyOn(console, level as 'log').mockImplementation(() => {}),
    );
    const user = userEvent.setup();
    open();
    await screen.findByRole('heading', { name: 'This link is protected' });

    // Act
    await user.type(screen.getByLabelText('Password'), 'right-one');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));
    await screen.findByRole('heading', { name: 'secret-plan.pdf' });

    // Assert
    expect(JSON.stringify({ ...localStorage })).not.toContain('right-one');
    expect(JSON.stringify({ ...sessionStorage })).not.toContain('right-one');
    expect(JSON.stringify(lines.map((spy) => spy.mock.calls))).not.toContain('right-one');
  });
});

describe('the public page: the rate limit (AC-9)', () => {
  it('counts down from Retry-After, keeps Unlock disabled with its reason, then enables it', async () => {
    // Arrange
    stubServer({
      [`GET ${SHARE}`]: problem(401, 'password_required'),
      [`POST ${SHARE}/password-verifications`]: problem(
        429,
        'rate_limited',
        { retry_after: 42 },
        { 'Retry-After': '42' },
      ),
    });
    const user = userEvent.setup();
    open();
    await screen.findByRole('heading', { name: 'This link is protected' });

    // Act
    await user.type(screen.getByLabelText('Password'), 'wrongpass1');
    await user.click(screen.getByRole('button', { name: 'Unlock' }));

    // Assert
    const message = await screen.findByText(/Too many attempts\./, { selector: 'strong' });
    expect(message.parentElement).toHaveTextContent('Too many attempts. Try again in 42 seconds.');
    const unlock = screen.getByRole('button', { name: 'Unlock' });
    expect(unlock).toBeDisabled();
    expect(unlock).toHaveAttribute('title', 'Wait until the countdown ends');
    expect(screen.getByText('Unlock is off until the countdown ends.')).toBeVisible();

    // Act (the countdown ends)
    vi.setSystemTime(new Date('2026-11-21T10:00:43.000Z'));

    // Assert
    await waitFor(() => expect(screen.getByRole('button', { name: 'Unlock' })).toBeEnabled(), {
      timeout: 3000,
    });
    expect(screen.queryByText(/Too many attempts/)).not.toBeInTheDocument();
  });

  it('shows the countdown and a disabled Try again when the first look is limited, then looks again', async () => {
    // Arrange
    let limited = true;
    const server = stubServer({
      [`GET ${SHARE}`]: () =>
        limited
          ? problem(429, 'rate_limited', { retry_after: 5 }, { 'Retry-After': '5' })
          : json(available()),
    });
    const user = userEvent.setup();

    // Act
    open();

    // Assert
    const message = await screen.findByText(/Too many attempts\./, { selector: 'strong' });
    expect(message.parentElement).toHaveTextContent('Try again in 5 seconds');
    expect(screen.getByRole('button', { name: 'Try again' })).toBeDisabled();
    expect(server.calls).toHaveLength(1);

    // Act (the countdown ends, the visitor looks again)
    limited = false;
    vi.setSystemTime(new Date('2026-11-21T10:00:06.000Z'));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Try again' })).toBeEnabled(), {
      timeout: 3000,
    });
    await user.click(screen.getByRole('button', { name: 'Try again' }));

    // Assert
    expect(await screen.findByRole('heading', { name: 'beach-sunset.jpg' })).toBeVisible();
  });
});

describe('the public page: the token (AC-10)', () => {
  it('removes the token from the address once the first answer is in and logs it nowhere', async () => {
    // Arrange
    stubServer({ [`GET ${SHARE}`]: json(available()) });
    const lines = ['log', 'info', 'warn', 'error', 'debug'].map((level) =>
      vi.spyOn(console, level as 'log').mockImplementation(() => {}),
    );

    // Act
    open();
    await screen.findByRole('heading', { name: 'beach-sunset.jpg' });

    // Assert
    await waitFor(() => expect(location.pathname).toBe('/s'));
    expect(location.href).not.toContain(TOKEN);
    expect(JSON.stringify(history.state)).not.toContain(TOKEN);
    expect(JSON.stringify(lines.map((spy) => spy.mock.calls))).not.toContain(TOKEN);
  });

  it('tells the browser to send no referrer while the page is open, and stops when it closes', async () => {
    // Arrange
    stubServer({ [`GET ${SHARE}`]: json(available()) });
    const meta = () => document.head.querySelector('meta[name="referrer"]');

    // Act
    const { unmount } = open();
    await screen.findByRole('heading', { name: 'beach-sunset.jpg' });

    // Assert
    expect(meta()).toHaveAttribute('content', 'no-referrer');

    // Act
    unmount();

    // Assert
    expect(meta()).toBeNull();
  });

  it('removes the token from the address for a failed link as well', async () => {
    // Arrange
    stubServer({ [`GET ${SHARE}`]: problem(404, 'not_found') });

    // Act
    open();
    await screen.findByRole('heading', { name: "This link isn't available" });

    // Assert
    await waitFor(() => expect(location.pathname).toBe('/s'));
    expect(location.href).not.toContain(TOKEN);
  });
});
