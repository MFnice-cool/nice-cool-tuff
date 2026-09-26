import { createClient, type Session, type User } from "@supabase/supabase-js";

/** Built-in TuffSwarm Supabase project (must match Rust `BUILTIN_SUPABASE_URL`). */
export const TUFFSWARM_SUPABASE_URL = "https://vsoqnwknpueuubiovyjd.supabase.co";
/**
 * Legacy anon JWT for Auth in the webview.
 * Publishable `sb_publishable_…` works for REST from Rust; GoTrue/sign-in is most
 * reliable with the JWT anon key in supabase-js.
 */
export const TUFFSWARM_SUPABASE_ANON_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InZzb3Fud2tucHVldXViaW92eWpkIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODQ4MTEwMDYsImV4cCI6MjEwMDM4NzAwNn0.E9L11ipWyNiSchUx6pxT3HOVxu_vHtYDUOnNTixqJaI";

/**
 * Landing page after email confirmation. Must be allowlisted in Supabase
 * Auth → URL Configuration → Redirect URLs (and preferred over Site URL
 * localhost:3000 so mail clients do not open a dead local server).
 */
export const AUTH_EMAIL_REDIRECT_TO =
  "https://cdn.jsdelivr.net/gh/MFcrychelt/tuffbox@master/docs/auth-confirmed.html";

export const supabase = createClient(
  TUFFSWARM_SUPABASE_URL,
  TUFFSWARM_SUPABASE_ANON_KEY,
  {
    auth: {
      persistSession: true,
      autoRefreshToken: true,
      detectSessionInUrl: false,
      storageKey: "tuffbox-crash-votes-auth",
    },
    global: {
      headers: {
        "X-Client-Info": "tuffbox-desktop-crash-votes",
      },
    },
  },
);

export type AuthSnapshot = {
  session: Session | null;
  user: User | null;
};

function authErrorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    const msg = String((err as { message: unknown }).message ?? "");
    if (/failed to fetch|networkerror|load failed|fetch/i.test(msg)) {
      return "Cannot reach Supabase Auth (network / CSP). Restart the app after updating TuffBox, then try again.";
    }
    if (msg.trim()) return msg;
  }
  return err instanceof Error ? err.message : String(err);
}

export async function getAuthSnapshot(): Promise<AuthSnapshot> {
  const { data, error } = await supabase.auth.getSession();
  if (error) throw new Error(authErrorMessage(error));
  return { session: data.session, user: data.session?.user ?? null };
}

export async function signUpWithEmail(email: string, password: string) {
  const { data, error } = await supabase.auth.signUp({
    email: email.trim(),
    password,
    options: {
      emailRedirectTo: AUTH_EMAIL_REDIRECT_TO,
    },
  });
  if (error) throw new Error(authErrorMessage(error));
  return data;
}

export async function signInWithEmail(email: string, password: string) {
  const { data, error } = await supabase.auth.signInWithPassword({
    email: email.trim(),
    password,
  });
  if (error) throw new Error(authErrorMessage(error));
  return data;
}

export async function signOut() {
  const { error } = await supabase.auth.signOut();
  if (error) throw new Error(authErrorMessage(error));
}
