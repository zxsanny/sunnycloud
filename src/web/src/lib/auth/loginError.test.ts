import { describe, expect, it } from 'vitest';
import { describeLoginError } from './loginError';

describe('describeLoginError', () => {
  it('says nothing when there is no error reason', () => {
    // Act
    const view = describeLoginError(null);

    // Assert
    expect(view).toBeNull();
  });

  it('shows the not-invited text for invitation_required', () => {
    // Act
    const view = describeLoginError('invitation_required');

    // Assert
    expect(view).toMatchObject({ variant: 'danger', title: 'This account is not invited.' });
  });

  it('shows the deactivated text for account_not_active', () => {
    // Act
    const view = describeLoginError('account_not_active');

    // Assert
    expect(view).toMatchObject({ variant: 'warning', title: 'This account is deactivated.' });
  });

  it('shows a neutral text for any other reason', () => {
    // Act
    const view = describeLoginError('some_future_reason');

    // Assert
    expect(view).toMatchObject({ title: 'Sign-in did not complete.' });
  });
});
