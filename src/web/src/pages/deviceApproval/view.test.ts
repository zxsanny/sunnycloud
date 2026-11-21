import { describe, expect, it } from 'vitest';
import { actionState, countdownText, placeText, remainingSeconds, secondsUntil } from './view';

describe('the action state of the approval page (AC-2)', () => {
  it('enables Approve and makes it primary when the places match', () => {
    // Act
    const state = actionState(false, false);

    // Assert
    expect(state).toEqual({ primary: 'approve', approveEnabled: true });
  });

  it('makes Deny primary and disables Approve on a mismatch until the box is ticked', () => {
    // Act
    const unticked = actionState(true, false);
    const ticked = actionState(true, true);

    // Assert
    expect(unticked).toEqual({ primary: 'deny', approveEnabled: false });
    expect(ticked).toEqual({ primary: 'deny', approveEnabled: true });
  });

  it('disables Approve again when a ticked box is unticked', () => {
    // Act
    const states = [true, false].map((confirmed) => actionState(true, confirmed).approveEnabled);

    // Assert
    expect(states).toEqual([true, false]);
  });
});

describe('the countdown of the approval page (AC-5)', () => {
  const now = Date.parse('2026-11-21T10:00:00Z');

  it('counts whole seconds to the expiry the server sent', () => {
    // Act
    const left = remainingSeconds('2026-11-21T10:10:00Z', now);

    // Assert
    expect(left).toBe(600);
    expect(countdownText(left)).toBe('10:00');
  });

  it('rounds a part of a second up and never goes below zero', () => {
    // Act
    const partial = secondsUntil(now + 1500, now);
    const past = remainingSeconds('2026-11-21T09:59:00Z', now);

    // Assert
    expect(partial).toBe(2);
    expect(past).toBe(0);
  });

  it('counts an unreadable expiry as over', () => {
    // Act
    const left = remainingSeconds('soon', now);

    // Assert
    expect(left).toBe(0);
  });

  it('writes seconds with two digits', () => {
    // Act
    const text = countdownText(65);

    // Assert
    expect(text).toBe('1:05');
  });
});

describe('the place text (AC-6)', () => {
  it('shows the location the server resolved', () => {
    // Act
    const text = placeText('Kyiv, Ukraine');

    // Assert
    expect(text).toBe('Kyiv, Ukraine');
  });

  it('says so when the location is unknown', () => {
    // Act
    const texts = [placeText(null), placeText('')];

    // Assert
    expect(texts).toEqual(['Location unknown', 'Location unknown']);
  });
});
