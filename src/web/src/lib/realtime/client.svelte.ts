import {
  decodeServerFrame,
  encodePing,
  encodeSubscribe,
  type ChangeFrame,
  type ResetFrame,
  type ThumbCode,
} from './frames';

export type ConnectionState =
  'idle' | 'connecting' | 'live' | 'reconnecting' | 'signed-out' | 'failed';

/** A position in the account's change feed: the cursor `E.S` of `GET /changes`. */
export interface ResumePoint {
  epoch: bigint;
  seq: bigint;
}

/** What the client asks of the app: the REST reads of the first load and the places frames go. */
export interface RealtimeHandlers {
  /** `GET /changes/head`: the resume point of a first load. Null when the read failed. */
  loadHead(): Promise<ResumePoint | null>;
  /** The REST listing of the open folder (and its thumbnail states). False when it failed. */
  loadListing(): Promise<boolean>;
  /** One `POST /thumbnail-states` for the nodes whose preview is still pending. */
  refreshPendingThumbs(): Promise<void>;
  onChange(frame: ChangeFrame): void;
  onThumbState(contentHash: string, state: ThumbCode): void;
  /** Close code 4401: the session ended. */
  onSignedOut(): void;
  /** Close code 1002: the two sides disagree about a frame; a bug, not a network problem. */
  onProtocolError(): void;
  /** Hello announced a protocol version this client does not know. */
  onUpgradeRequired(): void;
}

/** The members of a WebSocket the client uses; tests give it a fake. */
export interface SocketLike {
  binaryType: string;
  readyState: number;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: ((event: { code: number; reason?: string }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  send(data: Uint8Array): void;
  close(code?: number, reason?: string): void;
}

export interface RealtimeOptions {
  url: string;
  handlers: RealtimeHandlers;
  /** The WebSocket constructor; the default is the browser's. */
  socket?: (url: string) => SocketLike;
  random?: () => number;
}

const OPEN = 1;
const PROTOCOL_VERSION = 1;
/** A missing Pong this long after a Ping, or no Hello this long after the socket opened, means a dead socket. */
const ANSWER_TIMEOUT_MS = 10_000;
/** No inbound message for three ping intervals (the server's idle rule, 90 s) means a dead socket. */
const SILENCE_TIMEOUT_MS = 90_000;
const BACKOFF_BASE_MS = 1000;
const BACKOFF_MAX_MS = 30_000;

/** The delay before reconnect attempt `attempt` (0 for the first): 1 s doubling to 30 s, plus up to 25 % jitter. */
export function backoffDelay(attempt: number, random: () => number): number {
  const base = Math.min(BACKOFF_MAX_MS, BACKOFF_BASE_MS * 2 ** attempt);
  return Math.min(BACKOFF_MAX_MS, Math.round(base * (1 + random() * 0.25)));
}

export function realtimeUrl(location: { protocol: string; host: string }): string {
  return `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/api/v1/realtime`;
}

interface Connection {
  ws: SocketLike;
  /** Set when this side closes it, so the close event is not read as the server's. */
  closedByUs: boolean;
  subscribed: boolean;
  nonce: number;
  helloTimer: ReturnType<typeof setTimeout> | null;
  silenceTimer: ReturnType<typeof setTimeout> | null;
  pingTimer: ReturnType<typeof setTimeout> | null;
  pongTimer: ReturnType<typeof setTimeout> | null;
}

/**
 * The one WebSocket of the web app (contract realtime.md). Live state arrives only as pushed frames. The timers
 * here are this side's own keepalive and its reconnect backoff: nothing on a timer asks the server for data.
 *
 * Order of a first connection: open, wait for Hello, then the REST first load (the resume point first, then the
 * listing, so a change between the two is re-sent), then Subscribe at that resume point. After a drop it
 * subscribes at the last point it applied (a Change with `end`) and asks once for the pending thumbnail states.
 */
export class RealtimeClient {
  state = $state<ConnectionState>('idle');

  private conn: Connection | null = null;
  private running = false;
  private attempt = 0;
  private resume: ResumePoint | null = null;
  private everSubscribed = false;
  private backoffTimer: ReturnType<typeof setTimeout> | null = null;
  private resetting = false;
  private pendingReset: ResetFrame | null = null;
  private readonly random: () => number;
  private readonly open: (url: string) => SocketLike;

  constructor(private readonly options: RealtimeOptions) {
    this.random = options.random ?? Math.random;
    this.open = options.socket ?? ((url) => new WebSocket(url) as unknown as SocketLike);
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.attempt = 0;
    this.connect();
  }

  stop() {
    this.running = false;
    this.clearBackoff();
    this.dropConnection(1000, 'leaving');
    // Coming back later is a first connection again: the folder may be another one.
    this.resume = null;
    this.everSubscribed = false;
    this.state = 'idle';
  }

  private connect() {
    this.state = this.everSubscribed ? 'reconnecting' : 'connecting';
    const ws = this.open(this.options.url);
    ws.binaryType = 'arraybuffer';
    const conn: Connection = {
      ws,
      closedByUs: false,
      subscribed: false,
      nonce: 0,
      helloTimer: null,
      silenceTimer: null,
      pingTimer: null,
      pongTimer: null,
    };
    this.conn = conn;
    ws.onopen = () => {
      if (this.conn !== conn) return;
      conn.helloTimer = setTimeout(() => this.dead(conn, 'no Hello'), ANSWER_TIMEOUT_MS);
      this.armSilence(conn);
    };
    ws.onmessage = (event) => {
      if (this.conn !== conn) return;
      this.armSilence(conn);
      this.receive(conn, event.data);
    };
    ws.onclose = (event) => {
      if (this.conn !== conn) return;
      this.closed(conn, event.code);
    };
    // A failed handshake or a broken socket always ends in a close event, which does the work.
    ws.onerror = () => undefined;
  }

  private receive(conn: Connection, data: unknown) {
    if (!(data instanceof ArrayBuffer)) {
      this.protocolFault(conn, 'a message that is not binary');
      return;
    }
    const decoded = decodeServerFrame(data);
    if (!decoded.ok) {
      if (decoded.refusal.reason === 'unknown_type') {
        // A newer server may send frames this build does not know (contract): ignore them.
        console.warn('Ignoring a realtime frame of unknown type', decoded.refusal.type);
      } else {
        this.protocolFault(conn, JSON.stringify(decoded.refusal));
      }
      return;
    }
    const frame = decoded.frame;
    switch (frame.type) {
      case 'hello':
        this.hello(conn, frame.version, frame.pingIntervalS);
        break;
      case 'change':
        this.options.handlers.onChange(frame);
        // Half a batch must not move the resume point: only the last item of a `seq` does.
        if (frame.end && conn.subscribed) this.resume = { epoch: frame.epoch, seq: frame.seq };
        break;
      case 'thumbState':
        this.options.handlers.onThumbState(frame.contentHash, frame.state);
        break;
      case 'reset':
        void this.reset(conn, frame);
        break;
      case 'pong':
        if (frame.nonce === conn.nonce && conn.pongTimer !== null) {
          clearTimeout(conn.pongTimer);
          conn.pongTimer = null;
        }
        break;
    }
  }

  /** A frame the server should never send: close, and come back after the backoff like any other drop. */
  private protocolFault(conn: Connection, what: string) {
    console.warn('Closing the realtime socket: bad frame from the server', what);
    this.dead(conn, 'bad frame', 1002);
  }

  private hello(conn: Connection, version: number, pingIntervalS: number) {
    if (conn.helloTimer !== null) clearTimeout(conn.helloTimer);
    conn.helloTimer = null;
    if (version !== PROTOCOL_VERSION) {
      this.running = false;
      this.state = 'failed';
      this.dropConnection(1000, 'unknown protocol version');
      this.options.handlers.onUpgradeRequired();
      return;
    }
    this.attempt = 0;
    this.armPing(conn, pingIntervalS * 1000);
    void this.begin(conn);
  }

  private async begin(conn: Connection) {
    const { handlers } = this.options;
    const reconnect = this.everSubscribed;
    if (this.resume === null) {
      const head = await handlers.loadHead();
      if (this.conn !== conn) return;
      if (head === null || !(await handlers.loadListing()) || this.conn !== conn) {
        if (this.conn === conn) this.dead(conn, 'first load failed');
        return;
      }
      this.resume = head;
    }
    this.subscribe(conn, this.resume);
    if (reconnect) await handlers.refreshPendingThumbs();
  }

  private subscribe(conn: Connection, at: ResumePoint) {
    if (conn.ws.readyState !== OPEN) return;
    conn.ws.send(encodeSubscribe(at.epoch, at.seq));
    conn.subscribed = true;
    this.everSubscribed = true;
    this.state = 'live';
  }

  /** The listing is read once over REST and the subscription restarts at the head the Reset names. */
  private async reset(conn: Connection, frame: ResetFrame) {
    this.pendingReset = frame;
    if (this.resetting) return;
    this.resetting = true;
    try {
      while (this.pendingReset !== null) {
        const head = this.pendingReset;
        this.pendingReset = null;
        const loaded = await this.options.handlers.loadListing();
        if (this.conn !== conn) return;
        if (!loaded) {
          this.dead(conn, 'reload after Reset failed');
          return;
        }
        this.resume = { epoch: head.epoch, seq: head.seq };
        this.subscribe(conn, this.resume);
      }
    } finally {
      this.resetting = false;
    }
  }

  private armPing(conn: Connection, intervalMs: number) {
    conn.pingTimer = setTimeout(() => {
      if (this.conn !== conn || conn.ws.readyState !== OPEN) return;
      conn.nonce = (conn.nonce + 1) >>> 0;
      if (conn.pongTimer === null) {
        conn.pongTimer = setTimeout(() => this.dead(conn, 'no Pong'), ANSWER_TIMEOUT_MS);
      }
      conn.ws.send(encodePing(conn.nonce));
      this.armPing(conn, intervalMs);
    }, intervalMs);
  }

  private armSilence(conn: Connection) {
    if (conn.silenceTimer !== null) clearTimeout(conn.silenceTimer);
    conn.silenceTimer = setTimeout(() => this.dead(conn, 'silence'), SILENCE_TIMEOUT_MS);
  }

  /** The socket is not answering or sent garbage: leave it, do not wait for its close event, and reconnect. */
  private dead(conn: Connection, why: string, code = 1000) {
    if (this.conn !== conn) return;
    console.warn('The realtime socket is not usable:', why);
    this.clearTimers(conn);
    conn.closedByUs = true;
    this.conn = null;
    conn.ws.onopen = conn.ws.onmessage = conn.ws.onclose = null;
    conn.ws.close(code, why.slice(0, 100));
    if (this.running) this.scheduleReconnect();
  }

  private closed(conn: Connection, code: number) {
    this.clearTimers(conn);
    this.conn = null;
    if (!this.running) return;
    if (code === 4401) {
      this.running = false;
      this.state = 'signed-out';
      this.options.handlers.onSignedOut();
    } else if (code === 1002) {
      this.running = false;
      this.state = 'failed';
      this.options.handlers.onProtocolError();
    } else {
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect() {
    this.state = 'reconnecting';
    const delay = backoffDelay(this.attempt, this.random);
    this.attempt += 1;
    this.backoffTimer = setTimeout(() => {
      this.backoffTimer = null;
      if (this.running) this.connect();
    }, delay);
  }

  private clearBackoff() {
    if (this.backoffTimer !== null) clearTimeout(this.backoffTimer);
    this.backoffTimer = null;
  }

  private clearTimers(conn: Connection) {
    for (const timer of [conn.helloTimer, conn.silenceTimer, conn.pingTimer, conn.pongTimer]) {
      if (timer !== null) clearTimeout(timer);
    }
    conn.helloTimer = conn.silenceTimer = conn.pingTimer = conn.pongTimer = null;
  }

  private dropConnection(code: number, reason: string) {
    const conn = this.conn;
    if (conn === null) return;
    this.clearTimers(conn);
    conn.closedByUs = true;
    this.conn = null;
    conn.ws.onopen = conn.ws.onmessage = conn.ws.onclose = null;
    conn.ws.close(code, reason);
  }
}
