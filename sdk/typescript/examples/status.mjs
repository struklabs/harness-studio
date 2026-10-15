// Prints the running Harness Studio's version, its servers and the library.
//   node examples/status.mjs                     (after `npm run build`)
// Finds the instance through <data dir>/instance.json (HARNESS_DATA_DIR or the app's folder).
import { unwrap } from '../dist/index.js';
import { connect } from '../dist/node.js';

const client = connect();
const API = '/api/harness/v1';

const status = await unwrap(client.GET(`${API}/status`));
console.log(`Harness Studio ${status.version}: ${status.servers.running} of ${status.servers.total} servers running`);
for (const server of await unwrap(client.GET(`${API}/servers`))) {
  console.log(`server ${server.name}: ${server.status}${server.served_alias ? ` (${server.served_alias})` : ''}`);
}
const library = await unwrap(client.GET(`${API}/models`));
for (const entry of library.entries) console.log(`model ${entry.model.id}`);
for (const definition of library.definitions) console.log(`definition ${definition.alias}`);
