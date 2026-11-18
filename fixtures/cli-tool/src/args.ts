export interface CliArgs {
  verbose: boolean;
  path: string;
}

/** Parse process argv into structured args. */
export function parseArgs(argv: string[]): CliArgs {
  return { verbose: argv.includes("--verbose"), path: argv[argv.length - 1] ?? "." };
}
