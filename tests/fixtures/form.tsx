export const BigForm = () => (
	<form
		data-on:submit__prevent="@post('/api/submit')"
		data-indicator="submitting"
	>
		<fieldset data-attr:disabled="$submitting">
			<input
				class="input input-bordered w-full"
				type="text"
				placeholder="Name"
				data-bind="name"
			/>
			<input
				class="input input-bordered w-full"
				type="email"
				placeholder="Email"
				data-bind="email"
			/>
			<textarea
				class="textarea textarea-bordered w-full"
				placeholder="Message"
				data-bind="message"
			/>
			<button
				class="btn btn-primary"
				type="submit"
				data-show="!$submitting"
			>
				Submit
			</button>
			<span data-show="$submitting" class="loading loading-spinner" />
		</fieldset>
	</form>
);
