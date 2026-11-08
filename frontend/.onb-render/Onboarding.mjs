import * as $ from 'svelte/internal/server';

export default function Onboarding($$renderer, $$props) {
	$$renderer.component(($$renderer) => {
		/**
		 * A step, as data. The product decides what a step asks; this component decides only how a
		 * sequence of steps looks and behaves, so three sites can ask three different things and still
		 * feel like one family.
		 */
		/** Stable id, used for the DOM id and for the product's own bookkeeping. */
		/** One line: what this step is for. */
		/** Optional supporting sentence. */
		/** Zero-based index of the step being shown. */
		/** Called with the next index, or with the last index when the flow is finished. */
		/** Leaving is always allowed: an onboarding a person cannot escape is a wall, not a welcome. */
		/** Rendered only on the final step, where it replaces the advance button. */
		/** The step's own content. */
		let {
			steps,
			current,
			onAdvance,
			onSkip,
			finishLabel = 'Start',
			children
		} = $$props;

		let last = $.derived(() => Math.max(0, steps.length - 1));
		let isLast = $.derived(() => current >= last());
		let step = $.derived(() => steps[Math.min(Math.max(current, 0), last())]);

		function advance() {
			if (isLast()) onAdvance?.(current); else onAdvance?.(current + 1);
		}

		$$renderer.push(`<section class="onboarding svelte-1w213g1"${$.attr('aria-labelledby', `onboarding-${step().id}-title`)}><ol class="steps svelte-1w213g1"><!--[-->`);

		const each_array = $.ensure_array_like(steps);

		for (let i = 0, $$length = each_array.length; i < $$length; i++) {
			let s = each_array[i];

			$$renderer.push(`<li${$.attr('aria-current', i === current ? 'step' : undefined)}${$.attr_class('svelte-1w213g1', void 0, { 'current': i === current, 'done': i < current })}><span class="marker svelte-1w213g1">${$.escape(i + 1)}</span> <span class="visually-hidden svelte-1w213g1">Step ${$.escape(i + 1)} of ${$.escape(steps.length)}</span></li>`);
		}

		$$renderer.push(`<!--]--></ol> <div class="panel"${$.attr('id', `onboarding-${step().id}`)}><h2${$.attr('id', `onboarding-${step().id}-title`)}>${$.escape(step().title)}</h2> `);

		if (step().body) {
			$$renderer.push(`<!--[0--><p class="lede svelte-1w213g1">${$.escape(step().body)}</p>`);
		} else {
			$$renderer.push('<!--[-1-->');
		}

		$$renderer.push(`<!--]--> `);

		if (children) {
			$$renderer.push(`<!--[0--><div class="content">`);
			children($$renderer);
			$$renderer.push(`<!----></div>`);
		} else {
			$$renderer.push('<!--[-1-->');
		}

		$$renderer.push(`<!--]--></div> <div class="actions svelte-1w213g1">`);

		if (onSkip) {
			$$renderer.push(`<!--[0--><button type="button" class="skip">Skip for now</button>`);
		} else {
			$$renderer.push('<!--[-1-->');
		}

		$$renderer.push(`<!--]--> <button type="button" class="advance svelte-1w213g1">${$.escape(isLast() ? finishLabel : 'Continue')}</button></div> <p class="progress svelte-1w213g1">Step ${$.escape(current + 1)} of ${$.escape(steps.length)}</p></section>`);
	});
}