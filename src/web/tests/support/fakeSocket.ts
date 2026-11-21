import type { SocketLike } from '$lib/realtime';
import { encodePing } from '$lib/realtime';

/** A WebSocket the test drives by hand: it records what the client sends and delivers what the test says. */
export class FakeSocket implements SocketLike {
  static all: FakeSocket[] = [];
  binaryType = 'blob';
  readyState = 0;
  onopen: SocketLike['onopen'] = null;
  onmessage: SocketLike['onmessage'] = null;
  onclose: SocketLike['onclose'] = null;
  onerror: SocketLike['onerror'] = null;
  readonly sent: Uint8Array[] = [];
  closedWith: { code?: number; reason?: string } | null = null;
  /** Answers every Ping with a Pong of the same nonce, like the server. */
  autoPong = false;

  constructor(readonly url: string) {
    FakeSocket.all.push(this);
  }

  static reset() {
    FakeSocket.all = [];
  }

  static get last(): FakeSocket {
    const last = FakeSocket.all.at(-1);
    if (!last) throw new Error('no socket was opened');
    return last;
  }

  send(data: Uint8Array) {
    this.sent.push(data);
    if (this.autoPong && data[0] === 0x02) {
      const pong = Uint8Array.from(data);
      pong[0] = 0x24;
      this.deliver(pong);
    }
  }

  close(code?: number, reason?: string) {
    this.closedWith = { code, reason };
    this.readyState = 3;
  }

  /** The network accepted the connection. */
  open() {
    this.readyState = 1;
    this.onopen?.({});
  }

  deliver(bytes: Uint8Array) {
    this.onmessage?.({
      data: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    });
  }

  deliverText(text: string) {
    this.onmessage?.({ data: text });
  }

  /** The server (or the network) ends the connection with a close code. */
  serverClose(code: number) {
    this.readyState = 3;
    this.onclose?.({ code });
  }

  /** The frames this side sent, as hex, in order. */
  get sentHex(): string[] {
    return this.sent.map((b) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join(''));
  }
}

export const pingOf = (nonce: number) => encodePing(nonce);
