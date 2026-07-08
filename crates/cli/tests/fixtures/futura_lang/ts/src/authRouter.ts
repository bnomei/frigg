/** Secondary TS module for multi-file lang probes. */

import { UserService, type UserRecord } from "./userService";

export function registerAuthRoutes(users: UserService): void {
  // Marker string for search_text on the TypeScript lang board.
  const routeMarker = "FUTURA_LANG_TS_AUTH_ROUTE_MARKER";
  void routeMarker;
  void users;
}

export function requireUser(users: UserService, email: string): UserRecord {
  const found = users.findByEmail(email);
  if (!found) {
    throw new Error(`unknown user for email=${email}`);
  }
  return found;
}
