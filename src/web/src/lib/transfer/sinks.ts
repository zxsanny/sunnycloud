import type { Sink } from './download';
import type { DownloadSink } from './protocol';

/** A private browser file of an earlier download is kept at most this long (nothing tells the page when its save finished). */
export const STALE_AFTER_MS = 60 * 60 * 1000;
export const OPFS_PREFIX = 'dl-';

/** The Worker-only synchronous access handle (not in the DOM typings of this TypeScript version). */
interface SyncAccess {
  write(block: Uint8Array, options: { at: number }): number;
  flush(): void;
  close(): void;
}
interface SyncCapable {
  createSyncAccessHandle(): Promise<SyncAccess>;
}

/** The part of a directory handle the cleanup uses (the real one is `FileSystemDirectoryHandle`). */
export interface DirectoryLike {
  entries(): AsyncIterable<
    [string, { kind: string; getFile?: () => Promise<{ lastModified: number }> }]
  >;
  removeEntry(name: string): Promise<void>;
}

/** Removes private download files older than `STALE_AFTER_MS`. */
export async function removeStale(directory: DirectoryLike, now: number): Promise<void> {
  for await (const [name, entry] of directory.entries()) {
    if (!name.startsWith(OPFS_PREFIX) || entry.kind !== 'file' || entry.getFile === undefined)
      continue;
    const file = await entry.getFile();
    if (now - file.lastModified > STALE_AFTER_MS) await directory.removeEntry(name);
  }
}

/** Streams into a file the person chose in the save dialog; the browser keeps the bytes in a swap file until `close`. */
async function pickerSink(handle: FileSystemFileHandle): Promise<Sink> {
  const writable = await handle.createWritable();
  return {
    write: (block) => writable.write(block as Uint8Array<ArrayBuffer>),
    async commit() {
      await writable.close();
      return null;
    },
    async discard() {
      await writable.abort();
      // The dialog already created the (empty) file; remove it where the browser allows.
      const removable = handle as FileSystemFileHandle & { remove?: () => Promise<void> };
      await removable.remove?.();
    },
  };
}

/** Writes a private browser file (OPFS) with the Worker-only synchronous handle: no copy of the file in the heap. */
async function opfsSink(name: string): Promise<Sink> {
  const root = await navigator.storage.getDirectory();
  try {
    await removeStale(root as unknown as DirectoryLike, Date.now());
  } catch (cause) {
    // Old files only cost disk space; the new download goes on.
    console.warn('Old private download files could not be removed.', cause);
  }
  const handle = await root.getFileHandle(name, { create: true });
  const access = await (handle as unknown as SyncCapable).createSyncAccessHandle();
  let at = 0;
  return {
    async write(block) {
      const written = access.write(block, { at });
      if (written !== block.length)
        throw new Error('The browser stored fewer bytes than were written.');
      at += written;
    },
    async commit() {
      access.flush();
      access.close();
      return handle.getFile();
    },
    async discard() {
      access.close();
      await root.removeEntry(name);
    },
  };
}

export function openSink(spec: DownloadSink): Promise<Sink> {
  return spec.kind === 'picker' ? pickerSink(spec.handle) : opfsSink(spec.name);
}
