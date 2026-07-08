/** Multi-language gate fixture: TypeScript user service. */

export interface UserRecord {
  id: string;
  email: string;
  displayName: string;
}

export class UserService {
  constructor(private readonly store: Map<string, UserRecord>) {}

  createUser(email: string, displayName: string): UserRecord {
    const record: UserRecord = {
      id: `user-${this.store.size + 1}`,
      email,
      displayName,
    };
    this.store.set(record.id, record);
    return record;
  }

  findByEmail(email: string): UserRecord | undefined {
    for (const user of this.store.values()) {
      if (user.email === email) {
        return user;
      }
    }
    return undefined;
  }
}

export const FUTURA_LANG_TS_MARKER = "FUTURA_LANG_TS_USER_SERVICE_MARKER";
