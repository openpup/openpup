export function hashString(input: string): number {
  let hash = 0;
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 31 + input.charCodeAt(i)) >>> 0;
  }
  return hash;
}

export function hslToHex(h: number, s: number, l: number): string {
  const saturation = s / 100;
  const lightness = l / 100;
  const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const section = h / 60;
  const second = chroma * (1 - Math.abs((section % 2) - 1));
  const match = lightness - chroma / 2;

  let r = 0;
  let g = 0;
  let b = 0;

  if (section >= 0 && section < 1) [r, g, b] = [chroma, second, 0];
  else if (section < 2) [r, g, b] = [second, chroma, 0];
  else if (section < 3) [r, g, b] = [0, chroma, second];
  else if (section < 4) [r, g, b] = [0, second, chroma];
  else if (section < 5) [r, g, b] = [second, 0, chroma];
  else [r, g, b] = [chroma, 0, second];

  const toHex = (value: number) =>
    Math.round((value + match) * 255)
      .toString(16)
      .padStart(2, '0');

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

const PUP_COLOR_OVERRIDES: Record<string, string> = {
  alpha: '#1D9E75',
  you: '#BA7517',
};

export interface PupMeta {
  displayName: string;
  accentColor: string;
}

export type PupMetaByKey = Record<string, PupMeta>;

export interface PupMetaSource {
  key: string;
  display_name?: string;
}

export function pupAccentColor(pupKey: string): string {
  const normalizedKey = pupKey.trim().toLowerCase();
  if (PUP_COLOR_OVERRIDES[normalizedKey]) {
    return PUP_COLOR_OVERRIDES[normalizedKey];
  }

  const seed = hashString(normalizedKey);
  const hue = seed % 360;
  const saturation = 68 + ((seed >> 8) % 8);
  const lightness = 46 + ((seed >> 16) % 8);
  return hslToHex(hue, saturation, lightness);
}

export function pupTagStyle(pupKey: string): { background: string; color: string } {
  const accent = pupAccentColor(pupKey);
  return {
    background: `color-mix(in srgb, ${accent} 16%, transparent)`,
    color: accent,
  };
}

export function buildPupMetaByKey(pups: PupMetaSource[]): PupMetaByKey {
  return {
    alpha: {
      displayName: 'Alpha',
      accentColor: pupAccentColor('alpha'),
    },
    you: {
      displayName: 'You',
      accentColor: pupAccentColor('you'),
    },
    ...Object.fromEntries(
      pups.map((pup) => [
        pup.key,
        {
          displayName: pup.display_name || pup.key,
          accentColor: pupAccentColor(pup.key),
        },
      ])
    ),
  };
}
