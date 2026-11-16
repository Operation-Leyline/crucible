import { NotesClient } from "@example/weather";
import type * as models from "@example/weather";

// The client sends Crucible-Version: 1 on every request.
const client = new NotesClient({ endpoint: process.env.NOTES_ENDPOINT });

// Values from the model's example "Clear description".
const input: models.ClearInput = {
  description: null,
  other: "keep",
};

await client.clear(input);
