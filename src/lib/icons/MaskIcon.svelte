<script lang="ts">
  // A picture used as a one-colour icon in `currentColor` (CSS mask), sized like the Lucide icons
  // beside it: the Foundry picture, the mastered wreath for Mastery Helper.
  let {
    src,
    size = 24,
    scale = 1,
    class: className = "",
    ...rest
  }: { src: string; size?: number; scale?: number; class?: string; [key: string]: unknown } = $props();
</script>

<span class="mask-icon {className}" style="width: {size}px; height: {size}px; --src: url('{src}'); --scale: {scale}" {...rest}></span>

<style>
  .mask-icon {
    display: inline-block;
    background: currentColor;
    /* Drawn bigger or smaller without changing the space it takes in the row. */
    transform: scale(var(--scale));
    -webkit-mask: var(--src) center / contain no-repeat;
    mask: var(--src) center / contain no-repeat;
  }
</style>
