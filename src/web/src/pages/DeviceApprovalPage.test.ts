import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { account, json, problem, stubServer, type Reply } from '../../tests/support/http';
import { handleApiError } from '../app';
import { api } from '$lib/api';
import { notices } from '$lib/notices';
import { navigate, route } from '$lib/router';
import { session } from '$lib/session';
import DeviceApprovalPage from './DeviceApprovalPage.svelte';

const GRANT_ID = '0190aaaa-0000-7000-8000-0000000000c1';
const NOW = '2026-11-21T10:00:00.000Z';
const EXPIRES = '2026-11-21T10:10:00.000Z';
const GRANT_URL = 'GET /api/v1/device-grants';
const DECIDE_URL = `PATCH /api/v1/device-grants/${GRANT_ID}`;

const approval = (over: Record<string, unknown> = {}) => ({
  grant_id: GRANT_ID,
  expires_at: EXPIRES,
  device: 'zx-thinkpad',
  requester_ip: '93.175.12.4',
  requester_location: 'Kyiv, Ukraine',
  approver_ip: '93.175.12.9',
  approver_location: 'Kyiv, Ukraine',
  mismatch: false,
  ...over,
});

const elsewhere = (over: Record<string, unknown> = {}) =>
  approval({
    requester_ip: '185.107.56.3',
    requester_location: 'Amsterdam, Netherlands',
    mismatch: true,
    ...over,
  });

function serve(over: Record<string, Reply> = {}) {
  return stubServer({
    'GET /api/v1/accounts/me': json(account),
    [GRANT_URL]: json(approval()),
    [DECIDE_URL]: new Response(null, { status: 204 }),
    ...over,
  });
}

async function enterCode(user: ReturnType<typeof userEvent.setup>, code = 'WDJB-MJHT') {
  await user.type(await screen.findByLabelText('Code'), code);
  await user.click(screen.getByRole('button', { name: 'Continue' }));
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date(NOW));
  session.status = 'ready';
  session.account = null;
  api.setErrorHandler(() => {});
  navigate('/device', { replace: true });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  api.setErrorHandler(() => {});
  notices.dismiss('session');
});

describe('the approval page: both places side by side (AC-1)', () => {
  it('shows the device, both places with their addresses and the mismatch warning', async () => {
    // Arrange
    const server = serve({ [GRANT_URL]: json(elsewhere()) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);

    // Assert
    expect(await screen.findByText('Approve this sign-in?')).toBeInTheDocument();
    expect(screen.getByTestId('device-name')).toHaveTextContent('zx-thinkpad');
    expect(screen.getByTestId('requester-place')).toHaveTextContent('Amsterdam, Netherlands');
    expect(screen.getByTestId('requester-place')).toHaveTextContent('185.107.56.3');
    expect(screen.getByTestId('approver-place')).toHaveTextContent('Kyiv, Ukraine');
    expect(screen.getByTestId('approver-place')).toHaveTextContent('93.175.12.9');
    expect(screen.getByText('Different location')).toBeInTheDocument();
    expect(screen.getByText(/comes from a different place than you/)).toBeInTheDocument();
    const lookup = server.calls.find((call) => call.method === 'GET' && call.url.search !== '');
    expect(lookup?.url.searchParams.get('user_code')).toBe('WDJB-MJHT');
  });

  it('shows "Same location" and no warning for a grant from the approver\'s own place', async () => {
    // Arrange
    serve();
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);

    // Assert
    expect(await screen.findByText('Same location')).toBeInTheDocument();
    expect(screen.queryByText('Different location')).not.toBeInTheDocument();
    expect(screen.queryByText(/comes from a different place/)).not.toBeInTheDocument();
  });

  it('renders the device name sent by the device as text, never as markup', async () => {
    // Arrange
    serve({ [GRANT_URL]: json(approval({ device: '<img src=x onerror=alert(1)>' })) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);

    // Assert
    expect(await screen.findByTestId('device-name')).toHaveTextContent(
      '<img src=x onerror=alert(1)>',
    );
    expect(document.querySelector('img')).toBeNull();
  });
});

describe('the approval page: Deny is the default on a mismatch (AC-2)', () => {
  it('makes Deny request primary, keeps Approve off until the box is ticked and off again when unticked', async () => {
    // Arrange
    serve({ [GRANT_URL]: json(elsewhere()) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);
    const approve = await screen.findByRole('button', { name: 'Approve' });
    const box = screen.getByRole('checkbox', { name: 'I started this sign-in on that device' });

    // Assert (before the tick)
    expect(screen.getByRole('button', { name: 'Deny request' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Deny' })).not.toBeInTheDocument();
    expect(approve).toBeDisabled();
    expect(screen.getByText('Approve stays off until you tick this box.')).toBeInTheDocument();

    // Act and assert (ticked, then unticked)
    await user.click(box);
    expect(approve).toBeEnabled();
    await user.click(box);
    expect(approve).toBeDisabled();
  });

  it('sends one deny for the grant id the lookup returned and shows the denied page', async () => {
    // Arrange
    const server = serve({ [GRANT_URL]: json(elsewhere()) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Deny request' }));

    // Assert
    expect(await screen.findByText('Request denied')).toBeInTheDocument();
    expect(screen.getByText(/zx-thinkpad wasn't connected/)).toBeInTheDocument();
    const decisions = server.calls.filter((call) => call.method === 'PATCH');
    expect(decisions).toHaveLength(1);
    expect(decisions[0]?.url.pathname).toBe(`/api/v1/device-grants/${GRANT_ID}`);
    expect(decisions[0]?.body).toEqual({ decision: 'deny' });
  });

  it('approves after the tick', async () => {
    // Arrange
    const server = serve({ [GRANT_URL]: json(elsewhere()) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);
    await user.click(await screen.findByRole('checkbox'));

    // Act
    await user.click(screen.getByRole('button', { name: 'Approve' }));

    // Assert
    expect(await screen.findByText('zx-thinkpad is connected')).toBeInTheDocument();
    expect(server.calls.filter((call) => call.method === 'PATCH')).toHaveLength(1);
  });
});

describe('the approval page: Approve on a match (AC-3)', () => {
  it('approves without any confirmation and shows "<device> is connected"', async () => {
    // Arrange
    const server = serve();
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);
    const approve = await screen.findByRole('button', { name: 'Approve' });

    // Act
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    await user.click(approve);

    // Assert
    expect(await screen.findByText('zx-thinkpad is connected')).toBeInTheDocument();
    const decisions = server.calls.filter((call) => call.method === 'PATCH');
    expect(decisions).toHaveLength(1);
    expect(decisions[0]?.body).toEqual({ decision: 'approve' });
  });
});

describe('the approval page: one call per decision (AC-4)', () => {
  it.each([
    ['Approve', 'approve', approval()],
    ['Deny', 'deny', approval()],
    ['Deny request', 'deny', elsewhere()],
  ])('sends one call when %s is double-clicked', async (name, decision, grant) => {
    // Arrange
    let release: () => void = () => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const server = serve({
      [GRANT_URL]: json(grant),
      [DECIDE_URL]: async () => {
        await gate;
        return new Response(null, { status: 204 });
      },
    });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);
    const button = await screen.findByRole('button', { name });

    // Act
    await user.dblClick(button);
    release();

    // Assert
    await waitFor(() =>
      expect(screen.queryByText('Approve this sign-in?')).not.toBeInTheDocument(),
    );
    const decisions = server.calls.filter((call) => call.method === 'PATCH');
    expect(decisions).toHaveLength(1);
    expect(decisions[0]?.body).toEqual({ decision });
  });

  it('shows the refusal in place and keeps the one call that was sent', async () => {
    // Arrange
    const server = serve({ [DECIDE_URL]: problem(409, 'conflict') });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Approve' }));

    // Assert
    expect(await screen.findByTestId('decision-error')).toBeInTheDocument();
    expect(server.calls.filter((call) => call.method === 'PATCH')).toHaveLength(1);
  });
});

describe('the approval page: countdown and expiry (AC-5)', () => {
  it('counts down from the expiry the lookup returned', async () => {
    // Arrange
    serve({ [GRANT_URL]: json(approval({ expires_at: '2026-11-21T10:08:42.000Z' })) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);

    // Assert
    expect(await screen.findByTestId('countdown')).toHaveTextContent('Code expires in 8:42');
  });

  it('shows the expired page at 0 and sends nothing when the clock passes the expiry', async () => {
    // Arrange
    const server = serve();
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);
    expect(await screen.findByTestId('countdown')).toHaveTextContent('10:00');

    // Act
    vi.setSystemTime(new Date('2026-11-21T10:10:01.000Z'));

    // Assert
    expect(
      await screen.findByText("This code has expired or isn't valid", {}, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Approve' })).not.toBeInTheDocument();
    expect(server.calls.filter((call) => call.method === 'PATCH')).toHaveLength(0);
  });

  it('shows the expired page when the server refuses the decision as unknown', async () => {
    // Arrange
    serve({ [DECIDE_URL]: problem(404, 'not_found') });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Approve' }));

    // Assert
    expect(await screen.findByText("This code has expired or isn't valid")).toBeInTheDocument();
  });
});

describe('the approval page: wrong, used and unknown codes (AC-6)', () => {
  it.each([
    ['unknown, expired or already decided', problem(404, 'not_found')],
    ['not shaped like a code', problem(400, 'invalid_request')],
  ])('shows the expired message and grants nothing for a code that is %s', async (_, reply) => {
    // Arrange
    const server = serve({ [GRANT_URL]: reply });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user, 'ZZZZ-ZZZZ');

    // Assert
    expect(await screen.findByText("This code has expired or isn't valid")).toBeInTheDocument();
    expect(screen.getByText(/already used or denied can't be used again/)).toBeInTheDocument();
    expect(server.calls.filter((call) => call.method === 'PATCH')).toHaveLength(0);
  });

  it('returns to code entry from "Enter a new code"', async () => {
    // Arrange
    serve({ [GRANT_URL]: problem(404, 'not_found') });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user);

    // Act
    await user.click(await screen.findByRole('button', { name: 'Enter a new code' }));

    // Assert
    expect(await screen.findByLabelText('Code')).toHaveValue('');
  });

  it('shows an unknown location as unknown, with the warning when the addresses differ', async () => {
    // Arrange
    serve({
      [GRANT_URL]: json(elsewhere({ requester_location: null, approver_location: null })),
    });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);

    // Assert
    expect(await screen.findByTestId('requester-place')).toHaveTextContent('Location unknown');
    expect(screen.getByTestId('requester-place')).toHaveTextContent('185.107.56.3');
    expect(screen.getByTestId('approver-place')).toHaveTextContent('Location unknown');
    expect(screen.getByText(/comes from a different place than you/)).toBeInTheDocument();
  });
});

describe('the approval page: the wrong-code limit (AC-7)', () => {
  it('shows the 429 countdown, keeps Continue disabled until it ends, then enables it', async () => {
    // Arrange
    serve({ [GRANT_URL]: problem(429, 'rate_limited', {}, { 'Retry-After': '90' }) });
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user, 'ZZZZ-ZZZZ');

    // Assert
    expect(await screen.findByTestId('code-limit')).toHaveTextContent('Try again in 1:30');
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();

    // Act (the limit ends)
    vi.setSystemTime(new Date('2026-11-21T10:01:31.000Z'));

    // Assert
    await waitFor(() => expect(screen.queryByTestId('code-limit')).not.toBeInTheDocument(), {
      timeout: 3000,
    });
    expect(screen.getByRole('button', { name: 'Continue' })).toBeEnabled();
  });

  it('sends no lookup while the countdown runs', async () => {
    // Arrange
    const server = serve({
      [GRANT_URL]: problem(429, 'rate_limited', {}, { 'Retry-After': '90' }),
    });
    const user = userEvent.setup();
    render(DeviceApprovalPage);
    await enterCode(user, 'ZZZZ-ZZZZ');
    await screen.findByTestId('code-limit');

    // Act
    await user.type(screen.getByLabelText('Code'), '{Enter}');

    // Assert
    expect(
      server.calls.filter((call) => call.url.pathname === '/api/v1/device-grants'),
    ).toHaveLength(1);
  });
});

describe('the approval page: session only (AC-8)', () => {
  it('sends a signed-out visitor to sign-in with this page as the return path and decides nothing', async () => {
    // Arrange
    session.status = 'idle';
    api.setErrorHandler(handleApiError);
    const server = stubServer({
      'GET /api/v1/accounts/me': problem(401, 'unauthenticated'),
      'POST /api/v1/auth/session-renewals': problem(401, 'unauthenticated'),
    });

    // Act
    render(DeviceApprovalPage);

    // Assert
    await waitFor(() => expect(route.path).toBe('/login'));
    expect(route.query.get('return')).toBe('/device');
    expect(screen.queryByLabelText('Code')).not.toBeInTheDocument();
    expect(server.calls.some((call) => call.url.pathname.includes('device-grants'))).toBe(false);
  });
});

describe('the approval page: nothing is kept (constraints)', () => {
  it('writes no code or grant id to browser storage', async () => {
    // Arrange
    serve();
    const user = userEvent.setup();
    render(DeviceApprovalPage);

    // Act
    await enterCode(user);
    await user.click(await screen.findByRole('button', { name: 'Approve' }));
    await screen.findByText('zx-thinkpad is connected');

    // Assert
    expect(localStorage).toHaveLength(0);
    expect(sessionStorage).toHaveLength(0);
  });
});
