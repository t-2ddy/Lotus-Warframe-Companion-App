<script lang="ts">
  // A simple Orokin Ducat in Lucide's style (24×24, round stroke): the full ring, the centre bar
  // with its forked tip, and the two side blades, turned 180°. For the Ducanator tab.
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
  <!-- drawn upside down: forked tip at the bottom, blades flaring up -->
  <g transform="rotate(180 12 12)">
    <circle cx="12" cy="12" r="7" />
    <path d="M12 6v13" />
    <path d="M9 3l3 3 3-3" />
    <path d="M7 8v8l-2 4M17 8v8l2 4" />
  </g>
</svg>
