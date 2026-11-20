import { api, isAppLevelError, type ApiError } from '$lib/api';
import { formatBytes } from '$lib/format';
import { notices } from '$lib/notices';
import { navigate, route } from '$lib/router';
import { isPublicPath } from './routes';

function sessionEnded() {
  // The sign-in, invite and setup pages work without a session and show their own state.
  if (isPublicPath(route.path)) return;
  notices.show({
    key: 'session',
    variant: 'info',
    title: 'Your session ended.',
    text: 'Sign in again to continue.',
    dismissible: true,
  });
  navigate(`/login?return=${encodeURIComponent(route.relative)}`, { replace: true });
}

async function quotaReached() {
  const quota = await api.getAccountsMeQuota();
  const numbers = quota.ok
    ? ` You use ${formatBytes(quota.value.used)} of ${formatBytes(quota.value.effective_limit)}.`
    : '';
  notices.show({
    key: 'quota',
    variant: 'danger',
    title: 'Storage quota reached.',
    text: `${numbers.trim()} Delete files or ask your administrator for more space.`.trim(),
    dismissible: true,
  });
}

/** The one place where an API failure becomes a notice (contract api_client.md, mapping table). */
export function handleApiError(error: ApiError) {
  if (!isAppLevelError(error)) return;
  if (error.status === 401) {
    sessionEnded();
  } else if (error.status === 426) {
    notices.show({
      key: 'upgrade',
      variant: 'warning',
      title: 'A newer version of sunnycloud is required.',
      text: 'Reload the page to update the web app.',
      action: 'reload',
    });
  } else if (error.status === 429) {
    const seconds = error.retryAfterSeconds;
    notices.show({
      key: 'rate-limit',
      variant: 'warning',
      title: 'Too many requests.',
      text: 'Wait a moment and try again.',
      retryAt: seconds === null ? undefined : Date.now() + seconds * 1000,
    });
  } else if (error.status === 507) {
    void quotaReached();
  } else if (error.kind === 'network') {
    notices.show({
      key: 'network',
      variant: 'danger',
      title: 'Cannot reach the server.',
      text: 'Check your connection and try again.',
      dismissible: true,
    });
  } else {
    notices.show({
      key: 'server',
      variant: 'danger',
      title: 'Something went wrong on the server.',
      text: 'If it keeps happening, send this reference to your administrator:',
      requestId: error.requestId ?? undefined,
      dismissible: true,
    });
  }
}
