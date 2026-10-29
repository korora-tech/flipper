// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://flipper.korora.tech',
	trailingSlash: 'ignore',
	integrations: [
		starlight({
			title: 'Flipper',
			description: 'A Linux browser and profile picker that routes every link to the right browser profile.',
			logo: { src: './src/assets/logo.svg', alt: '' },
			favicon: '/favicon.svg',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/korora-tech/flipper' }],
			editLink: { baseUrl: 'https://github.com/korora-tech/flipper/edit/main/website/' },
			customCss: ['./src/styles/starlight.css'],
			sidebar: [
				{
					label: 'Getting started',
					items: [
						{ label: 'Installation', slug: 'docs/installation' },
						{ label: 'Using the picker', slug: 'docs/picker' },
					],
				},
				{
					label: 'Configuration',
					items: [
						{ label: 'Configuration file', slug: 'docs/configuration' },
						{ label: 'Routing rules', slug: 'docs/rules' },
						{ label: 'Browsers and profiles', slug: 'docs/profiles' },
						{ label: 'Local overrides', slug: 'docs/overrides' },
					],
				},
				{
					label: 'Integrations',
					items: [
						{ label: 'Home Manager', slug: 'docs/home-manager' },
						{ label: 'Browser extensions', slug: 'docs/browser-extensions' },
					],
				},
				{
					label: 'Reference',
					items: [{ label: 'Command line', slug: 'docs/cli' }],
				},
			],
		}),
	],
});
