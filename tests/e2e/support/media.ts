import { execFile } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { createReadStream, createWriteStream } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);

export type Expect = "image" | "video" | "none";

export interface MediaFile {
  name: string;
  path: string;
  expect: Expect;
  /** Pixel width of the source, for images the preview (at most 1440 px wide) is compared against it. */
  width?: number;
  sha256: string;
  bytes: number;
}

/** A hostile name: markup and quote characters, no slash (a slash is not a legal file name). */
export const HOSTILE_NAME = `<img src=x onerror=window.__xss=1>"'&.txt`;
export const LARGE_MIB = 40;

async function noise(dir: string, name: string, width: number, height: number, sigma: number): Promise<string> {
  const bands = await Promise.all(
    [0, 1, 2].map(async (band) => {
      const path = join(dir, `${name}-${band}.v`);
      await run("vips", ["gaussnoise", path, String(width), String(height), "--mean", "128", "--sigma", String(sigma)]);
      return path;
    }),
  );
  const joined = join(dir, `${name}.v`);
  await run("vips", ["bandjoin", bands.join(" "), joined]);
  return joined;
}

export async function writeRandom(path: string, mib: number): Promise<void> {
  const out = createWriteStream(path);
  for (let i = 0; i < mib; i++) {
    if (!out.write(randomBytes(1024 * 1024))) await new Promise<void>((resolve) => out.once("drain", () => resolve()));
  }
  await new Promise<void>((resolve, reject) => out.end((error?: Error | null) => (error ? reject(error) : resolve())));
}

function hashFile(path: string): Promise<{ sha256: string; bytes: number }> {
  return new Promise((resolve, reject) => {
    const hash = createHash("sha256");
    let bytes = 0;
    createReadStream(path)
      .on("data", (chunk) => {
        hash.update(chunk);
        bytes += chunk.length;
      })
      .on("error", reject)
      .on("end", () => resolve({ sha256: hash.digest("hex"), bytes }));
  });
}

/** Generates the media set into `dir`: three image formats, a HEIC, an MP4, a MOV, a 40 MiB blob, a text file with a hostile name. */
export async function generateMedia(dir: string): Promise<MediaFile[]> {
  await mkdir(dir, { recursive: true });
  const work = join(dir, ".work");
  await mkdir(work, { recursive: true });
  const big = await noise(work, "big", 3000, 2000, 40);
  const small = await noise(work, "small", 800, 600, 25);
  await run("vips", ["jpegsave", big, join(dir, "photo.jpg"), "--Q", "90"]);
  await run("vips", ["copy", small, join(dir, "drawing.png")]);
  await run("vips", ["webpsave", small, join(dir, "picture.webp"), "--Q", "40"]);
  await run("vips", ["heifsave", big, join(dir, "shot.heic"), "--Q", "50"]);
  const ffmpeg = ["-hide_banner", "-loglevel", "error", "-y"];
  await run("ffmpeg", [...ffmpeg, "-f", "lavfi", "-i", "testsrc=duration=3:size=640x360:rate=25", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", join(dir, "clip.mp4")]);
  await run("ffmpeg", [...ffmpeg, "-f", "lavfi", "-i", "testsrc2=duration=3:size=480x270:rate=30", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-f", "mov", join(dir, "movie.mov")]);
  await writeRandom(join(dir, "large.bin"), LARGE_MIB);
  await writeFile(join(dir, HOSTILE_NAME), "not markup\n");

  const entries: [string, Expect, number | undefined][] = [
    ["photo.jpg", "image", 3000],
    ["drawing.png", "image", 800],
    ["picture.webp", "image", 800],
    ["shot.heic", "image", 3000],
    ["clip.mp4", "video", undefined],
    ["movie.mov", "video", undefined],
    ["large.bin", "none", undefined],
    [HOSTILE_NAME, "none", undefined],
  ];
  return Promise.all(
    entries.map(async ([name, expect, width]) => {
      const path = join(dir, name);
      return { name, path, expect, width, ...(await hashFile(path)) };
    }),
  );
}

export { hashFile };
