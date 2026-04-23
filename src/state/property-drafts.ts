import type {
  PropertySection,
  WallpaperProperty,
  WallpaperRuntimeRecord,
} from "../types";

export type DraftPropertyValues = Record<string, unknown>;
type ConditionTokenType = "identifier" | "number" | "string" | "boolean" | "operator" | "paren";

interface ConditionToken {
  type: ConditionTokenType;
  value: string;
}

export function propertyMap(wallpaper?: WallpaperRuntimeRecord | null) {
  return new Map((wallpaper?.propertySchema ?? []).map((property) => [property.key, property.value]));
}

export function serializePropertyValue(value: unknown) {
  if (typeof value === "string") {
    return value;
  }
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

export function applyDraftsToWallpaper(
  wallpaper?: WallpaperRuntimeRecord | null,
  draftValues: DraftPropertyValues = {},
) {
  if (!wallpaper) {
    return wallpaper ?? null;
  }
  if (Object.keys(draftValues).length === 0) {
    return wallpaper;
  }

  return {
    ...wallpaper,
    propertySchema: wallpaper.propertySchema.map((property) =>
      Object.prototype.hasOwnProperty.call(draftValues, property.key)
        ? { ...property, value: draftValues[property.key] }
        : property,
    ),
  };
}

function propertyNumberSuffix(key: string, prefix: string) {
  const suffix = key.startsWith(prefix) ? key.slice(prefix.length) : null;
  if (!suffix) {
    return null;
  }
  const parsed = Number.parseInt(suffix, 10);
  return Number.isFinite(parsed) ? parsed : null;
}

function fallbackInspectorLabel(key: string, label: string) {
  if (!label.trim()) {
    const userTextureNumber = propertyNumberSuffix(key, "appdockapplyusertexture");
    if (userTextureNumber !== null) {
      return `自定义图标 #${userTextureNumber}`;
    }
    const enableNumber = propertyNumberSuffix(key, "appdockenable");
    if (enableNumber !== null) {
      return `启用图标 #${enableNumber}`;
    }
    return key;
  }
  return label.trim();
}

function propertyLooksDecorative(property: WallpaperProperty) {
  if (property.presentation === "decoration") {
    return true;
  }
  const key = property.key.toLowerCase();
  const label = property.label.toLowerCase();
  return (
    key.startsWith("imgsrc")
    || key.startsWith("brimgsrc")
    || key.includes("imgsrchttp")
    || key.includes("hrefhttps")
    || key.includes("<img")
    || label.includes("<img")
    || label.includes("<a ")
    || label.includes("<hr")
    || label.includes("rf=viewer")
    || (property.key.length > 96 && !property.key.includes("_"))
  );
}

function needsInspectorGroupContext(label: string) {
  const compact = label.toLowerCase().replace(/\s+/g, "").replace("／", "/");
  return [
    "大小size",
    "颜色color",
    "位置xpositionx",
    "位置ypositiony",
    "文本位置xpositionx",
    "文本位置ypositiony",
    "opacity",
    "musicsize",
  ].includes(compact);
}

export function deriveInspectorProperties(properties: WallpaperProperty[]) {
  const normalized = properties.map((property) => {
    const label = fallbackInspectorLabel(property.key, property.label);
    return {
      ...property,
      label,
      presentation:
        propertyLooksDecorative(property) ? "decoration" : property.presentation,
    } satisfies WallpaperProperty;
  });

  let currentGroup: string | null = null;
  return normalized.map((property) => {
    if (property.presentation === "group") {
      currentGroup = property.label;
      return property;
    }
    if (
      property.presentation === "control"
      && currentGroup
      && needsInspectorGroupContext(property.label)
      && !property.label.startsWith(currentGroup)
    ) {
      return {
        ...property,
        label: `${currentGroup} / ${property.label}`,
      };
    }
    return property;
  });
}

export function buildPropertyMapByKey(properties: WallpaperProperty[]) {
  return new Map(properties.map((property) => [property.key, property]));
}

export function sectionScopedLabel(section: PropertySection, property: WallpaperProperty) {
  const prefix = `${section.label} / `;
  if (property.label.startsWith(prefix)) {
    return property.label.slice(prefix.length);
  }
  return property.label;
}

function tokenizeCondition(input: string) {
  const tokens: ConditionToken[] = [];
  let index = 0;

  while (index < input.length) {
    const char = input[index];
    if (/\s/.test(char)) {
      index += 1;
      continue;
    }

    const two = input.slice(index, index + 2);
    if (["&&", "||", "==", "!=", ">=", "<="].includes(two)) {
      tokens.push({ type: "operator", value: two });
      index += 2;
      continue;
    }

    if ([">", "<", "!"].includes(char)) {
      tokens.push({ type: "operator", value: char });
      index += 1;
      continue;
    }

    if (char === "(" || char === ")") {
      tokens.push({ type: "paren", value: char });
      index += 1;
      continue;
    }

    if (char === "'" || char === "\"") {
      let cursor = index + 1;
      let value = "";
      while (cursor < input.length && input[cursor] !== char) {
        value += input[cursor];
        cursor += 1;
      }
      if (cursor >= input.length) {
        return null;
      }
      tokens.push({ type: "string", value });
      index = cursor + 1;
      continue;
    }

    if (/[0-9.-]/.test(char)) {
      const match = input.slice(index).match(/^-?\d+(?:\.\d+)?/);
      if (!match) {
        return null;
      }
      tokens.push({ type: "number", value: match[0] });
      index += match[0].length;
      continue;
    }

    const identifier = input.slice(index).match(/^[A-Za-z_][A-Za-z0-9_.]*/);
    if (!identifier) {
      return null;
    }
    const value = identifier[0];
    tokens.push({
      type: value === "true" || value === "false" ? "boolean" : "identifier",
      value,
    });
    index += value.length;
  }

  return tokens;
}

function normalizeComparable(value: unknown): unknown {
  if (typeof value === "string") {
    const trimmed = value.trim();
    const lowered = trimmed.toLowerCase();
    if (["true", "on"].includes(lowered)) {
      return true;
    }
    if (["false", "off"].includes(lowered)) {
      return false;
    }
    if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) {
      return Number(trimmed);
    }
    return trimmed;
  }
  return value;
}

export function valuesEqual(left: unknown, right: unknown) {
  const normalizedLeft = normalizeComparable(left);
  const normalizedRight = normalizeComparable(right);
  return Object.is(normalizedLeft, normalizedRight);
}

export function truthy(value: unknown) {
  const normalized = normalizeComparable(value);
  if (typeof normalized === "boolean") {
    return normalized;
  }
  if (typeof normalized === "number") {
    return normalized !== 0;
  }
  if (typeof normalized === "string") {
    if (!normalized) {
      return false;
    }
    return !["0", "none"].includes(normalized.toLowerCase());
  }
  return Boolean(normalized);
}

export function evaluatePropertyCondition(
  expression: string | null | undefined,
  properties: Map<string, unknown>,
) {
  if (!expression?.trim()) {
    return true;
  }

  const tokens = tokenizeCondition(expression);
  if (!tokens) {
    return null;
  }

  let index = 0;

  const parsePrimary = (): unknown => {
    const token = tokens[index];
    if (!token) {
      throw new Error("Unexpected end of expression");
    }
    if (token.type === "operator" && token.value === "!") {
      index += 1;
      return !truthy(parsePrimary());
    }
    if (token.type === "paren" && token.value === "(") {
      index += 1;
      const value = parseOr();
      const closing = tokens[index];
      if (!closing || closing.type !== "paren" || closing.value !== ")") {
        throw new Error("Missing closing parenthesis");
      }
      index += 1;
      return value;
    }
    index += 1;
    if (token.type === "boolean") {
      return token.value === "true";
    }
    if (token.type === "number") {
      return Number(token.value);
    }
    if (token.type === "string") {
      return token.value;
    }
    if (token.type === "identifier") {
      const key = token.value.endsWith(".value")
        ? token.value.slice(0, -".value".length)
        : token.value;
      return properties.get(key);
    }
    throw new Error(`Unexpected token ${token.value}`);
  };

  const parseComparison = (): unknown => {
    let left = parsePrimary();
    while (
      tokens[index]?.type === "operator"
      && ["==", "!=", ">", ">=", "<", "<="].includes(tokens[index].value)
    ) {
      const operator = tokens[index].value;
      index += 1;
      const right = parsePrimary();
      switch (operator) {
        case "==":
          left = valuesEqual(left, right);
          break;
        case "!=":
          left = !valuesEqual(left, right);
          break;
        case ">":
          left = Number(left) > Number(right);
          break;
        case ">=":
          left = Number(left) >= Number(right);
          break;
        case "<":
          left = Number(left) < Number(right);
          break;
        case "<=":
          left = Number(left) <= Number(right);
          break;
      }
    }
    return left;
  };

  const parseAnd = (): unknown => {
    let left = parseComparison();
    while (tokens[index]?.type === "operator" && tokens[index].value === "&&") {
      index += 1;
      left = truthy(left) && truthy(parseComparison());
    }
    return left;
  };

  const parseOr = (): unknown => {
    let left = parseAnd();
    while (tokens[index]?.type === "operator" && tokens[index].value === "||") {
      index += 1;
      left = truthy(left) || truthy(parseAnd());
    }
    return left;
  };

  try {
    const result = parseOr();
    if (index < tokens.length) {
      throw new Error("Unexpected trailing tokens");
    }
    return truthy(result);
  } catch {
    return null;
  }
}

export function propertyVisible(property: WallpaperProperty, properties: Map<string, unknown>) {
  const result = evaluatePropertyCondition(property.condition, properties);
  return result !== false;
}

export function sectionVisible(
  section: PropertySection,
  propertiesByKey: Map<string, WallpaperProperty>,
  properties: Map<string, unknown>,
) {
  if (evaluatePropertyCondition(section.condition, properties) === false) {
    return false;
  }

  return section.items.some((item) => {
    if (item.kind === "property") {
      const property = item.key ? propertiesByKey.get(item.key) : null;
      return Boolean(property && propertyVisible(property, properties));
    }
    if (item.kind === "description") {
      return Boolean(item.text?.trim());
    }
    return item.kind === "separator";
  });
}

export function visibleSectionControlCount(
  sections: PropertySection[],
  propertiesByKey: Map<string, WallpaperProperty>,
  properties: Map<string, unknown>,
) {
  return sections.reduce((count, section) => {
    if (!sectionVisible(section, propertiesByKey, properties)) {
      return count;
    }
    return (
      count
      + section.items.filter((item) => {
        if (item.kind !== "property" || !item.key) {
          return false;
        }
        const property = propertiesByKey.get(item.key);
        return Boolean(property && propertyVisible(property, properties));
      }).length
    );
  }, 0);
}
