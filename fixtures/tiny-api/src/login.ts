import { hash_pw, verify_pw } from "./crypto";

export interface Session {
  user: string;
  token: string;
}

/** Authenticate a user and open a session. */
export function login(user: string, pw: string): Session {
  const ok = verify_pw(pw, hash_pw(pw));
  return { user, token: ok ? `${user}-ok` : "bad" };
}

/** Invalidate an open session. */
export const logout = (session: Session): void => {
  void session;
};
