// Loads a model through the Control API and waits for it, printing server status events.
//   node examples/load-model.mjs <definition alias>
import { streamEvents, unwrap } from '../dist/index.js';
import { connect } from '../dist/node.js';

const alias = process.argv[2];
if (!alias) {
  console.error('usage: node examples/load-model.mjs <definition alias>');
  process.exit(2);
}
const client = connect();
const controller = new AbortController();
const watching = (async () => {
  for await (const { type, event } of streamEvents(client, { types: ['server_status'], signal: controller.signal })) {
    console.log(type, JSON.stringify(event));
  }
})().catch((e) => {
  if (e.name !== 'AbortError') throw e;
});
const loaded = await unwrap(client.POST('/api/harness/v1/load', { body: { alias } }));
console.log('loaded:', JSON.stringify(loaded));
controller.abort();
await watching;
