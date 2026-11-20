<script lang="ts" module>
  import type { Component } from 'svelte';

  export type PageComponent = Component;
</script>

<script lang="ts">
  import { matchRoute, type RouteDef } from './match';
  import { route } from './router.svelte';

  let { routes, notFound }: { routes: RouteDef<PageComponent>[]; notFound: PageComponent } =
    $props();

  const Page = $derived(matchRoute(routes, route.path)?.component ?? notFound);
</script>

<Page />
