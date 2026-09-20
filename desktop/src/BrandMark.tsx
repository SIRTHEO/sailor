/**
 * The mark of one brand: the brand's own drawing where the build has one, and a
 * monogram otherwise — so a command line is legible the day a descriptor names
 * it, not the day somebody draws it.
 */
import { MARKS, hueOfSlug, legible, monogramOf } from "./brands";

interface BrandMarkProps {
  slug: string;
  label: string;
  size?: number;
}

export function BrandMark({ slug, label, size = 16 }: BrandMarkProps) {
  const mark = MARKS[slug];
  const side = { width: size, height: size, flex: "0 0 auto" as const };

  if (mark === undefined) {
    return (
      <span
        className="brand brand--letter"
        style={{ ...side, background: hueOfSlug(slug || label), fontSize: size * 0.56 }}
        role="img"
        aria-label={label}
        data-brand={slug || "unnamed"}
      >
        {monogramOf(label)}
      </span>
    );
  }

  return (
    <svg
      className="brand"
      style={side}
      viewBox="0 0 24 24"
      role="img"
      aria-label={mark.title}
      data-brand={slug}
      fill={legible(mark.hex)}
    >
      <path d={mark.path} />
    </svg>
  );
}
