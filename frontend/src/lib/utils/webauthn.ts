// src/lib/utils/webauthn.ts

export async function isBiometricAvailable(): Promise<boolean> {
  if (!window.PublicKeyCredential) return false;
  return await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
}

export async function registerBiometrics(): Promise<string | null> {
  try {
    const challenge = new Uint8Array(32);
    crypto.getRandomValues(challenge);

    const credential = (await navigator.credentials.create({
      publicKey: {
        challenge,
        rp: { name: "Vault Private Space", id: window.location.hostname },
        user: {
          id: new TextEncoder().encode("vault-admin"),
          name: "admin@vault",
          displayName: "Vault Admin",
        },
        pubKeyCredParams: [{ alg: -7, type: "public-key" }, { alg: -257, type: "public-key" }],
        authenticatorSelection: {
          authenticatorAttachment: "platform", // TouchID, Windows Hello, FaceID
          userVerification: "required",
        },
        timeout: 60000,
      },
    })) as PublicKeyCredential;

    if (!credential) return null;
    const credId = btoa(String.fromCharCode(...new Uint8Array(credential.rawId)));

    await fetch("/api/vault/register-biometric", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ credential_id: credId }),
    });

    return credId;
  } catch (err) {
    console.warn("Biometric enrollment bypassed or failed:", err);
    return null;
  }
}

export async function authenticateBiometrics(): Promise<boolean> {
  try {
    const challenge = new Uint8Array(32);
    crypto.getRandomValues(challenge);

    const assertion = await navigator.credentials.get({
      publicKey: {
        challenge,
        timeout: 60000,
        userVerification: "required",
        rpId: window.location.hostname,
      },
    });

    return Boolean(assertion);
  } catch (err) {
    console.error("Biometric verification failed:", err);
    return false;
  }
}