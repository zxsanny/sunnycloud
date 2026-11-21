import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { transfer, type JobProgress } from '$lib/transfer';
import TransfersPanel from './TransfersPanel.svelte';

afterEach(() => {
  transfer.jobs = [];
});

const row = (extra: Partial<JobProgress>): JobProgress => ({
  id: 'dn-1',
  direction: 'download',
  name: 'backup-2026.tar',
  parentId: '',
  size: 3_000_000_000,
  state: 'done',
  phase: null,
  bytesDone: 3_000_000_000,
  lanes: 0,
  ...extra,
});

describe('the transfers panel: download outcomes', () => {
  it('shows the verified label of the design and no hash line', () => {
    // Arrange
    transfer.jobs = [
      row({
        outcome: { kind: 'VerifiedSaved' },
        note: 'Downloaded and checked in the browser.',
      }),
    ];

    // Act
    render(TransfersPanel, { onselect: () => {} });

    // Assert
    expect(screen.getByText('Verified')).toBeInTheDocument();
    expect(screen.getByText('Downloaded and checked in the browser.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Copy file hash' })).toBeNull();
  });

  it('shows "not verified" with the server hash and a copy action that copies exactly that hash', async () => {
    // Arrange
    const hash = '4f2a9c0e7d1b'.padEnd(64, '5');
    transfer.jobs = [
      row({
        outcome: { kind: 'SavedUnverified', reason: 'OverBrowserVerifyLimit', serverHash: hash },
        note: 'Saved by the browser without a check (over 2 GB in this browser).',
      }),
    ];
    const user = userEvent.setup();
    render(TransfersPanel, { onselect: () => {} });

    // Act
    await user.click(screen.getByRole('button', { name: 'Copy file hash' }));

    // Assert
    expect(screen.getByText('Not verified')).toBeInTheDocument();
    expect(
      screen.getByText(/Saved by the browser without a check \(over 2 GB in this browser\)\./),
    ).toHaveTextContent('Compare its hash:');
    expect(screen.getByText(hash)).toHaveClass('font-code');
    expect(await navigator.clipboard.readText()).toBe(hash);
  });

  it('shows no hash line when the server gave no hash', () => {
    // Arrange
    transfer.jobs = [
      row({
        outcome: { kind: 'SavedUnverified', reason: 'NoServerHash', serverHash: null },
        note: 'Saved by the browser without a check (the server gives no file hash for this link).',
      }),
    ];

    // Act
    render(TransfersPanel, { onselect: () => {} });

    // Assert
    expect(screen.getByText('Not verified')).toBeInTheDocument();
    expect(screen.queryByText(/Compare its hash/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Copy file hash' })).toBeNull();
  });

  it('shows a hash mismatch as a failure, without a verified or retry control', () => {
    // Arrange
    transfer.jobs = [
      row({
        state: 'failed',
        outcome: { kind: 'Failed', reason: 'HashMismatch' },
        errorText: 'Not saved: the downloaded bytes did not match the file on the server.',
      }),
    ];

    // Act
    render(TransfersPanel, { onselect: () => {} });

    // Assert
    expect(screen.getByText(/did not match the file on the server/)).toBeInTheDocument();
    expect(screen.queryByText('Verified')).toBeNull();
    expect(screen.queryByRole('button', { name: /Retry/ })).toBeNull();
    expect(screen.getByRole('button', { name: 'Dismiss backup-2026.tar' })).toBeInTheDocument();
  });
});
