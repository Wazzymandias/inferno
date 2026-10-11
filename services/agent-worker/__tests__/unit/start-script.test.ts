import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const projectRoot = resolve(import.meta.dirname, '../..');

interface PackageManifest {
  readonly scripts: Readonly<Record<string, string>>;
}

function readStartEntry(): string {
  const manifest = JSON.parse(
    readFileSync(resolve(projectRoot, 'package.json'), 'utf8'),
  ) as PackageManifest;
  const match = /^node (\S+)$/.exec(manifest.scripts.start ?? '');
  if (match?.[1] === undefined) {
    throw new Error(
      `Unexpected start script: ${String(manifest.scripts.start)}`,
    );
  }
  return resolve(projectRoot, match[1]);
}

// tsconfig.json owns the emit layout (rootDir/outDir); ask the compiler where
// src/main.ts lands instead of restating that layout here.
function emittedEntry(): string {
  const parsed = ts.getParsedCommandLineOfConfigFile(
    resolve(projectRoot, 'tsconfig.json'),
    {},
    {
      ...ts.sys,
      onUnRecoverableConfigFileDiagnostic: (diagnostic) => {
        throw new Error(
          ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
        );
      },
    },
  );
  if (parsed === undefined) {
    throw new Error('Unable to parse tsconfig.json');
  }
  const js = ts
    .getOutputFileNames(parsed, resolve(projectRoot, 'src/main.ts'), false)
    .find((file) => file.endsWith('.js'));
  if (js === undefined) {
    throw new Error('tsconfig.json emits no JavaScript for src/main.ts');
  }
  return resolve(js);
}

describe('start script', () => {
  it('runs the file tsc emits for src/main.ts', () => {
    expect(readStartEntry()).toBe(emittedEntry());
  });
});
