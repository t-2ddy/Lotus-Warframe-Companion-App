<script lang="ts">
  // The Chat tab's icon in Lucide's style: a hollow square cut in half vertically, each half
  // ending in an arrowhead so the two run counter-clockwise (left half down and out along the
  // bottom, right half up and back along the top), like a trade going both ways.
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
  <!-- left half: top → left side → bottom, arrow pointing right -->
  <path d="M10.5 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h4.5" />
  <path d="M8 17.5l2.5 2.5L8 22.5" />
  <!-- right half: bottom → right side → top, arrow pointing left -->
  <path d="M13.5 20H18a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-4.5" />
  <path d="M16 1.5L13.5 4 16 6.5" />
</svg>
