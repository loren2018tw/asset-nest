import type { RouteRecordRaw } from "vue-router";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    component: () => import("@/layouts/MainLayout.vue"),
    children: [
      { path: "", component: () => import("@/pages/IndexPage.vue") },
      { path: "assets", component: () => import("@/pages/AssetsPage.vue") },
      { path: "ips", component: () => import("@/pages/SubnetsPage.vue") },
      {
        path: "subnets/:id/ips",
        component: () => import("@/pages/IpListPage.vue")
      },
      {
        path: "kea/leases",
        component: () => import("@/pages/KeaLeasesPage.vue")
      },
      {
        path: "kea/status",
        component: () => import("@/pages/KeaStatusPage.vue")
      }
    ]
  },

  // Always leave this as last one,
  // but you can also remove it
  {
    path: "/:catchAll(.*)*",
    component: () => import("@/pages/ErrorNotFound.vue")
  }
];

export default routes;
