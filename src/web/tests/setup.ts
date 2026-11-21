import '@testing-library/jest-dom/vitest';
import { beforeEach, vi } from 'vitest';
import { FakeSocket } from './support/fakeSocket';

// No test opens a real socket: every WebSocket is the hand-driven fake (tests/support/fakeSocket.ts). Stubbed before
// each test because tests call vi.unstubAllGlobals().
beforeEach(() => {
  vi.stubGlobal('WebSocket', FakeSocket);
});
