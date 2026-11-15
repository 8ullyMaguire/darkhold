import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import Combobox from './Combobox.svelte';

describe('Combobox', () => {
	const options = [
		{ value: 'apple', label: 'Apple' },
		{ value: 'banana', label: 'Banana' },
		{ value: 'cherry', label: 'Cherry', disabled: true }
	];

	it('renders an editable combobox linked to a datalist of options', () => {
		render(Combobox, {
			props: { label: 'Fruit', options, value: '', onchange: vi.fn() }
		});

		const input = screen.getByRole('combobox');
		expect(input).toBeInTheDocument();
		// The datalist below the input is `Fruit-list`, so BOTH attributes must name that id. The test used to assert
		// `list="Fruit"` and `aria-controls="Fruit"`, which is what made the suggestions never bind: it was asserting
		// the bug (15235de6 fixed the component and left this file alone, and the tree carried one red test from then
		// on -- found by the gate sweep after merging the two darkhold lineages, 2026-11-15).
		expect(input).toHaveAttribute('list', 'Fruit-list');
		expect(input).toHaveAttribute('aria-controls', 'Fruit-list');
		expect(input).toHaveAttribute('aria-autocomplete', 'list');
		// aria-expanded is not allowed on a textbox (axe: aria-allowed-attr on /search): native <input list> needs no
		// ARIA to be accessible, and this asserts the removal rather than trusting the comment that says so.
		expect(input).not.toHaveAttribute('aria-expanded');
	});

	it('updates value and calls onchange when the user types', async () => {
		const onchange = vi.fn();
		render(Combobox, {
			props: { label: 'Fruit', options, value: '', onchange }
		});
		const input = screen.getByRole('combobox') as HTMLInputElement;

		await fireEvent.input(input, { target: { value: 'apple' } });
		expect(onchange).toHaveBeenCalledWith('apple');

		await fireEvent.input(input, { target: { value: 'banana' } });
		expect(onchange).toHaveBeenCalledWith('banana');
	});

	it('shows hint text when provided', () => {
		render(Combobox, {
			props: { label: 'Fruit', options, value: '', onchange: vi.fn(), hint: 'Select a fruit' }
		});
		expect(screen.getByText('Select a fruit')).toBeInTheDocument();
	});

	it('shows error text when provided', () => {
		render(Combobox, {
			props: { label: 'Fruit', options, value: 'apple', onchange: vi.fn(), error: 'Invalid choice' }
		});
		expect(screen.getByText('Invalid choice')).toBeInTheDocument();
	});

	it('combines hint and error in aria-describedby when both are present', () => {
		render(Combobox, {
			props: { label: 'Fruit', options, value: 'apple', onchange: vi.fn(), hint: 'Select a fruit', error: 'Invalid choice' }
		});
		const input = screen.getByRole('combobox');
		// When both are present, both IDs should be in aria-describedby
		const describedBy = input.getAttribute('aria-describedby');
		expect(describedBy).toContain('Fruit-helptext');
		expect(describedBy).toContain('Fruit-error');
	});
});