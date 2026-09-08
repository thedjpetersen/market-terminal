import { createRemoteJWKSet, jwtVerify, type JWTVerifyGetKey } from "jose";

export interface Session {
  principal: string;
  email: string;
}

type AuthConfig = {
  [
    K in keyof Pick<Env, "ACCESS_ISSUER" | "ACCESS_AUD" | "ACCESS_EMAILS">
  ]: string;
};

// Cache public signing keys only; authenticated users are always request-local.
const keySets = new Map<string, JWTVerifyGetKey>();
export async function authenticate(
  request: Request,
  env: AuthConfig,
  keys?: JWTVerifyGetKey,
): Promise<Session | null> {
  const token = request.headers.get("Cf-Access-Jwt-Assertion");
  if (!token || token.length > 16384 || !env.ACCESS_ISSUER || !env.ACCESS_AUD)
    return null;
  try {
    if (!keys) {
      keys = keySets.get(env.ACCESS_ISSUER);
      if (!keys) {
        keys = createRemoteJWKSet(
          new URL(`${env.ACCESS_ISSUER}/cdn-cgi/access/certs`),
          {
            timeoutDuration: 5000,
          },
        );
        keySets.set(env.ACCESS_ISSUER, keys);
      }
    }
    const { payload } = await jwtVerify(token, keys, {
      issuer: env.ACCESS_ISSUER,
      audience: env.ACCESS_AUD,
      algorithms: ["RS256"],
      requiredClaims: ["sub", "exp", "iat", "email"],
    });
    if (
      typeof payload.sub !== "string" ||
      !/^[a-zA-Z0-9._:-]{1,64}$/.test(payload.sub) ||
      typeof payload.email !== "string" ||
      !env.ACCESS_EMAILS.split(",")
        .map((email) => email.trim().toLowerCase())
        .includes(payload.email.toLowerCase())
    )
      return null;
    return { principal: payload.sub, email: payload.email };
  } catch {
    return null;
  }
}
