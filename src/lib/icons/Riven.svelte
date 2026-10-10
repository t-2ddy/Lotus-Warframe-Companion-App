<script lang="ts">
  // The riven symbol in Lucide's style, for the Rivens tab: a large downward triangle, a medium
  // upward one whose tip stops at the top edge, and a small filled triangle inside that. The game
  // art blurs at 16px.
  let {
    size = 24,
    strokeWidth = 2,
    class: className = "",
    ...rest
  }: { size?: number; strokeWidth?: number; class?: string; [key: string]: unknown } = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  class={className}
  {...rest}
>
  <path d="M2 4h20L12 22z" />
  <path d="M12 4l7 12H5z" />
  <path d="M12 9.5l2.6 4.5H9.4z" fill="currentColor" />
</svg>
