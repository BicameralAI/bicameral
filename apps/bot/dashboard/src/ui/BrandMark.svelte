<script lang="ts">
  /**
   * Brand marks for Settings connection rows.
   * Assets: dashboard/src/assets/brands/ (see README there).
   */
  import driveUrl from "../assets/brands/googledrive.svg?url";
  import claudeUrl from "../assets/brands/claude.svg?url";
  import codexUrl from "../assets/brands/codex.svg?url";

  type Brand = "google-drive" | "claude" | "codex";

  type Props = {
    brand: Brand;
    size?: number;
    title?: string;
  };

  let { brand, size = 28, title }: Props = $props();

  const meta = $derived.by(() => {
    switch (brand) {
      case "google-drive":
        return { src: driveUrl, label: title ?? "Google Drive" };
      case "claude":
        return { src: claudeUrl, label: title ?? "Claude" };
      case "codex":
        return { src: codexUrl, label: title ?? "Codex" };
    }
  });
</script>

<span
  class="brand-mark"
  class:drive={brand === "google-drive"}
  class:claude={brand === "claude"}
  class:codex={brand === "codex"}
  style={`width:${size}px;height:${size}px`}
  role="img"
  aria-label={meta.label}
  title={meta.label}
>
  <img src={meta.src} alt="" width={size * 0.72} height={size * 0.72} />
</span>

<style>
  .brand-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    border-radius: var(--radius-md);
    border: 1px solid var(--line);
    background: var(--surface);
    overflow: hidden;
  }
  .brand-mark.claude {
    background: #faf6f3;
  }
  .brand-mark.codex {
    background: var(--canvas-muted);
  }
  .brand-mark img {
    display: block;
    object-fit: contain;
  }
</style>
