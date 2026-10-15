import type { ClientEndpoint, GatewayOverview } from '#lib/types.js';

/** Only a live backend endpoint identifies this instance's gateway; settings are not an address. */
export function runningGatewayEndpoint(gateway: GatewayOverview | null): ClientEndpoint | null {
  return gateway?.status.running ? gateway.status.endpoint : null;
}
