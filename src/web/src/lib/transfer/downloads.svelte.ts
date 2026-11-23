import { contentUrl } from '$lib/files';
import type { ApiError, ContentRoute } from '$lib/api';
import { downloadFailureOf } from './download';
import {
  HASH_VERIFY_MAX_FF_SAFARI,
  chooseStrategy,
  type DownloadOutcome,
  type DownloadTarget,
  type DownloadEnv,
  type Strategy,
} from './downloadPlan';
import { DOWNLOAD_FAILURE_TEXT, VERIFIED_TEXT, unverifiedText } from './errors';
import type { JobProgress } from './jobs';
import type { DownloadFailure, DownloadSink, FromWorker } from './protocol';
import { UploadQueue } from './queue.svelte';
import { OPFS_PREFIX } from './sinks';

/** What a download needs before it starts: the file's name, size, the server's file hash and where its bytes are. */
interface Source {
  name: string;
  size: number;
  /** The server's file hash (roll-up of the block hashes), or null when the API gives none. */
  hash: string | null;
  /** The ranged routes the Worker reads; null when there is no hash to check against. */
  route: ContentRoute | null;
  /** The plain URL for the browser's own download. */
  url: string;
}

type Answer = Extract<FromWorker, { type: 'downloadDone' | 'downloadFailed' }>;

const failed = (reason: DownloadFailure): DownloadOutcome => ({ kind: 'Failed', reason });
const failureOfApi = (error: ApiError) => failed(downloadFailureOf(error));

/**
 * Downloads (contract transfer_service.md): the strategy by browser and size, the verified read in the Worker, and the
 * labelled outcome in the same panel rows as uploads.
 */
export class DownloadQueue extends UploadQueue {
  // Plain map: no template reads it.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  private readonly downloadAnswers = new Map<string, (answer: Answer) => void>();

  /** Never rejects. Every call ends with a row in the panel that carries the outcome. */
  async download(target: DownloadTarget): Promise<DownloadOutcome> {
    const row = this.addDownloadRow();
    const env = this.deps.download;
    if (env === undefined) {
      console.error('Downloads are not set up: the browser capabilities were not provided.');
      return this.end(row, failed('SaveFailed'));
    }
    const source = await this.resolve(target);
    if ('kind' in source) return this.end(row, source);
    row.name = source.name;
    row.size = source.size;
    const strategy = await this.strategyFor(env, source);
    switch (strategy.kind) {
      case 'native':
        env.saveFile(source.url, source.name);
        return this.end(row, {
          kind: 'SavedUnverified',
          reason: strategy.reason,
          serverHash: source.hash,
        });
      case 'picker':
        return this.end(row, await this.saveWithPicker(env, row, source));
      case 'opfs':
        return this.end(
          row,
          await this.verify(
            row,
            source,
            { kind: 'opfs', name: `${OPFS_PREFIX}${crypto.randomUUID()}` },
            env,
          ),
        );
    }
  }

  private async strategyFor(env: DownloadEnv, source: Source): Promise<Strategy> {
    if (source.hash === null || source.route === null) {
      return { kind: 'native', reason: 'NoServerHash' };
    }
    const strategy = chooseStrategy(
      env,
      source.size,
      this.deps.hashVerifyMax ?? HASH_VERIFY_MAX_FF_SAFARI,
    );
    if (strategy.kind !== 'opfs') return strategy;
    const free = await env.freeStorage();
    return free !== null && free < source.size
      ? { kind: 'native', reason: 'NotEnoughBrowserStorage' }
      : strategy;
  }

  private async saveWithPicker(
    env: DownloadEnv,
    row: JobProgress,
    source: Source,
  ): Promise<DownloadOutcome> {
    let handle: FileSystemFileHandle;
    try {
      handle = await (env.pickFile as NonNullable<DownloadEnv['pickFile']>)(source.name);
    } catch (cause) {
      if (cause instanceof DOMException && cause.name === 'AbortError') return failed('Cancelled');
      console.error('The save dialog could not be opened.', cause);
      return failed('SaveFailed');
    }
    return this.verify(row, source, { kind: 'picker', handle }, env);
  }

  /** The Worker reads, hashes and writes; the outcome is `VerifiedSaved` only after the hashes matched. */
  private async verify(
    row: JobProgress,
    source: Source,
    sink: DownloadSink,
    env: DownloadEnv,
  ): Promise<DownloadOutcome> {
    row.state = 'running';
    const answer = await new Promise<Answer>((resolve) => {
      this.downloadAnswers.set(row.id, resolve);
      this.send({
        type: 'download',
        job: row.id,
        source: source.route as ContentRoute,
        size: source.size,
        hash: source.hash as string,
        sink,
      });
    });
    if (answer.type === 'downloadFailed') return failed(answer.reason);
    if (sink.kind === 'opfs') {
      if (answer.file === undefined) return failed('SaveFailed');
      env.saveFile(answer.file, source.name);
    }
    return { kind: 'VerifiedSaved' };
  }

  private async resolve(target: DownloadTarget): Promise<Source | DownloadOutcome> {
    const { api } = this.deps;
    if ('shareToken' in target) {
      const share = await api.getPublicShare(target.shareToken);
      if (!share.ok) return failureOfApi(share.error);
      return {
        name: share.value.name,
        size: share.value.size,
        hash: share.value.content_hash,
        route: { kind: 'share', id: target.shareToken },
        url: `/api/v1/public-shares/${encodeURIComponent(target.shareToken)}/content`,
      };
    }
    const node = await api.getNode(target.nodeId);
    if (!node.ok) return failureOfApi(node.error);
    if (node.value.kind !== 'file' || node.value.state !== 'live') return failed('NotFound');
    const name = node.value.name;
    if (target.revisionId === undefined) {
      return {
        name,
        size: node.value.size ?? 0,
        hash: node.value.content_hash ?? null,
        route: { kind: 'node', id: target.nodeId },
        url: contentUrl(target.nodeId),
      };
    }
    const revision = await api.getRevision(target.revisionId);
    if (!revision.ok) return failureOfApi(revision.error);
    if (revision.value.node_id !== target.nodeId) return failed('NotFound');
    return {
      name,
      size: revision.value.size,
      hash: revision.value.content_hash,
      route: { kind: 'revision', id: target.revisionId },
      url: `/api/v1/revisions/${encodeURIComponent(target.revisionId)}/content`,
    };
  }

  private addDownloadRow(): JobProgress {
    const id = `dn-${(this.counter += 1)}`;
    this.jobs.push({
      id,
      direction: 'download',
      name: 'Download',
      parentId: '',
      size: 0,
      state: 'running',
      phase: null,
      bytesDone: 0,
      lanes: 0,
    });
    return this.jobs[this.jobs.length - 1] as JobProgress;
  }

  private end(row: JobProgress, outcome: DownloadOutcome): DownloadOutcome {
    row.outcome = outcome;
    this.downloadAnswers.delete(row.id);
    if (outcome.kind === 'Failed') {
      row.state = outcome.reason === 'Cancelled' ? 'cancelled' : 'failed';
      row.errorText = DOWNLOAD_FAILURE_TEXT[outcome.reason];
    } else {
      row.state = 'done';
      row.bytesDone = row.size;
      row.note =
        outcome.kind === 'VerifiedSaved'
          ? VERIFIED_TEXT
          : unverifiedText(outcome.reason, this.deps.hashVerifyMax ?? HASH_VERIFY_MAX_FF_SAFARI);
    }
    return outcome;
  }

  protected override receive(m: FromWorker) {
    if (m.type === 'downloadProgress') {
      const row = this.jobs.find((r) => r.id === m.job);
      if (row) row.bytesDone = m.bytesDone;
      return;
    }
    if (m.type === 'downloadDone' || m.type === 'downloadFailed') {
      this.downloadAnswers.get(m.job)?.(m);
      this.downloadAnswers.delete(m.job);
      return;
    }
    super.receive(m);
  }

  /** The page is going away and takes its Worker with it: a download that is running ends as not finished. */
  protected failDownloads() {
    for (const [id, answer] of [...this.downloadAnswers]) {
      answer({ type: 'downloadFailed', job: id, reason: 'Network' });
    }
    this.downloadAnswers.clear();
  }

  protected override failAllActive() {
    this.failDownloads();
    super.failAllActive();
  }
}
