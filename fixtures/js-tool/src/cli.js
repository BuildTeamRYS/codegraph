import { clamp } from './math.mjs';
import { padNum } from './format.cjs';

/**
 * Entry: read argv[2] as a level, clamp it to 1..5, print it zero-padded.
 */
export function main(argv) {
  const level = clamp(Number(argv[2] ?? 1), 1, 5);
  process.stdout.write(padNum(level, 2) + '\n');
  return level;
}

main(process.argv);
