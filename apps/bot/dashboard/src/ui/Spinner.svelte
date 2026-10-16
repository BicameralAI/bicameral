<script lang="ts">
  type Props = { size?: number; label?: string };
  let { size = 16, label = "Loading" }: Props = $props();
</script>

<span
  class="spinner"
  style:width="{size}px"
  style:height="{size}px"
  role="status"
  aria-label={label}
></span>

<style>
  .spinner {
    display: inline-block;
    border: 2px solid var(--line);
    border-top-color: var(--accent-500);
    border-radius: 50%;
    animation: spin 0.65s linear infinite;
    flex-shrink: 0;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
