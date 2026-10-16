<script lang="ts">
  type Props = { width?: string; height?: string };
  let { width = "100%", height = "0.75rem" }: Props = $props();
</script>

<span class="sk" style:width style:height aria-hidden="true"></span>

<style>
  .sk {
    display: inline-block;
    border-radius: var(--radius-sm);
    background: linear-gradient(
      90deg,
      var(--canvas-muted) 25%,
      var(--line) 50%,
      var(--canvas-muted) 75%
    );
    background-size: 200% 100%;
    animation: sh 1.2s ease-in-out infinite;
  }
  @keyframes sh {
    0% {
      background-position: 200% 0;
    }
    100% {
      background-position: -200% 0;
    }
  }
</style>
