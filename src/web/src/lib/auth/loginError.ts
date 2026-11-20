export interface LoginErrorView {
  variant: 'danger' | 'warning';
  title: string;
  text: string;
}

/**
 * The sign-in page's `?error=` reason, set by the server's callback redirect (api_v1_surface.md, open point 39).
 * `account_not_active` is the code of a deactivated account; any other reason gets the neutral text.
 */
export function describeLoginError(reason: string | null): LoginErrorView | null {
  if (reason === null || reason === '') return null;
  if (reason === 'invitation_required') {
    return {
      variant: 'danger',
      title: 'This account is not invited.',
      text: 'No account was created. Ask your administrator for an invite link, then sign in again.',
    };
  }
  if (reason === 'account_not_active') {
    return {
      variant: 'warning',
      title: 'This account is deactivated.',
      text: 'Ask your administrator to turn it back on.',
    };
  }
  return {
    variant: 'danger',
    title: 'Sign-in did not complete.',
    text: 'Try again. If it keeps happening, ask your administrator.',
  };
}
