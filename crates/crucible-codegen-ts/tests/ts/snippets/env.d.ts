// What a snippet reads from Node, declared so the check needs no
// @types/node install: snippets read their endpoint and token from the
// environment.
declare const process: { readonly env: Readonly<Record<string, string | undefined>> };
