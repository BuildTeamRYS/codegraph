import { parseArgs } from "./args";

const VERSION = "0.1.0";

/** Entry point. */
export function main(argv: string[]): number {
  const args = parseArgs(argv);
  console.log(`cli-tool ${VERSION} at ${args.path}`);
  return 0;
}

const run = async (): Promise<void> => {
  main(process.argv.slice(2));
};

void run;
