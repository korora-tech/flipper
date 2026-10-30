# flipper.korora.tech

The Flipper website: a landing page and the documentation, built with
[Astro](https://astro.build) and [Starlight](https://starlight.astro.build),
and served as static assets by a Cloudflare Worker.

```sh
npm ci
npm run dev       # http://localhost:4321
npm run build     # type-check and build into dist/
npm run preview   # serve the site locally with cf dev
npm run deploy    # build, then deploy with cf
```

Node 24 is expected (`.node-version`); Astro needs at least 22.22.

- `src/pages/index.astro` is the landing page. `src/components/Picker.astro`
  recreates the link picker from `src/ui.rs`; keep its labels and colours in
  step with the app.
- `src/content/docs/docs/` holds the documentation, served under `/docs/`.
  The sidebar is set in `astro.config.mjs`.
- `cloudflare.config.ts` serves the site on the custom domain
  `flipper.korora.tech`. Custom domains need the `korora.tech` zone in the same
  Cloudflare account; cf creates the DNS record and certificate on first
  deploy.

## Deployment

`.github/workflows/website.yaml` builds every pull request that touches
`website/` and deploys from `main` with `cf deploy`. The deploy job runs in the
`production` environment and logs in to Infisical over OIDC as this
repository's identity, which reads the Korora Tech Cloudflare token
(`CLOUDFLARE_API_TOKEN`). The identity, its environment and its
`INFISICAL_MACHINE_IDENTITY_ID` secret are managed in
[korora-tech/korora-tech](https://github.com/korora-tech/korora-tech).
