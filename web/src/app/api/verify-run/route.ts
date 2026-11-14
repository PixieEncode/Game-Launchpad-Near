import { NextResponse } from "next/server";
import { z } from "zod";
import { verifyRun } from "@/lib/verify-run";

export const runtime = "nodejs";

const account = z.string().regex(/^[a-z0-9._-]{2,64}$/);
const body = z.object({
  game: account,
  player: account,
  sessionId: z.number().int().nonnegative(),
  seed: z.number().int().nonnegative().max(0xffffffff),
  config: z.record(z.string(), z.unknown()),
  cosmeticsHash: z.string().regex(/^[0-9a-f]{64}$/),
  log: z.string().max(400_000),
});

/**
 * Verifies a finished run and returns the signature for `end_run`.
 * Nothing here is trusted by the contract: it checks the signature, the
 * session's seed and player, the game's config hash and the run's timing
 * itself, so a wrong field here only produces a signature the game refuses.
 */
export async function POST(req: Request) {
  const parsed = body.safeParse(await req.json().catch(() => null));
  if (!parsed.success) return NextResponse.json({ error: "Invalid request" }, { status: 400 });
  try {
    return NextResponse.json(await verifyRun(parsed.data));
  } catch (e) {
    return NextResponse.json({ error: e instanceof Error ? e.message : String(e) }, { status: 422 });
  }
}
