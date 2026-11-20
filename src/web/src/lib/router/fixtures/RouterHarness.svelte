<script lang="ts">
  import { Router, type PageComponent } from '$lib/router';
  import StubHome from './StubHome.svelte';
  import StubLogin from './StubLogin.svelte';
  import StubNotFound from './StubNotFound.svelte';

  const routes = [
    { path: '/', component: StubHome as PageComponent },
    { path: '/login', component: StubLogin as PageComponent },
  ];
</script>

<Router {routes} notFound={StubNotFound as PageComponent} />
