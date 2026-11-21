<script>
  import { ICONS } from '../lib/icons.js';
  let { name, size = 24 } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="currentColor"
  aria-hidden="true"
  focusable="false"
>
  <path d={ICONS[name] || ''} />
</svg>
