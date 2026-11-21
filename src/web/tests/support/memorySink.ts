import type { Sink } from '$lib/transfer/download';

/** A sink that records what the engine does with it; the Node stand-in for the save dialog's file or OPFS. */
export class MemorySink implements Sink {
  readonly blocks: Uint8Array[] = [];
  committed = false;
  discarded = false;
  failWrites = false;

  async write(block: Uint8Array): Promise<void> {
    if (this.failWrites) throw new DOMException('disk full', 'QuotaExceededError');
    // The engine reuses its block buffer, so a sink that keeps bytes copies them.
    this.blocks.push(block.slice());
  }

  async commit(): Promise<File | null> {
    this.committed = true;
    return new File(this.blocks as BlobPart[], 'saved.bin');
  }

  async discard(): Promise<void> {
    this.discarded = true;
  }

  get bytes(): Uint8Array {
    return Buffer.concat(this.blocks);
  }
}
