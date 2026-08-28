#ifndef ETHEREAL_MANAGER_CERT_H
#define ETHEREAL_MANAGER_CERT_H

#define ETHEREAL_MANAGER_TOKEN_SIZE 32
#define ETHEREAL_MANAGER_TOKEN_HEX_SIZE (ETHEREAL_MANAGER_TOKEN_SIZE * 2)

static int ethereal_hex_nibble(char c)
{
	if (c >= '0' && c <= '9')
		return c - '0';
	if (c >= 'a' && c <= 'f')
		return c - 'a' + 10;
	if (c >= 'A' && c <= 'F')
		return c - 'A' + 10;
	return -1;
}

static bool ethereal_manager_token_decode(
	const char *hex, u8 out[ETHEREAL_MANAGER_TOKEN_SIZE])
{
	unsigned int i;
	u8 any = 0;

	if (!hex || !out)
		return false;
	for (i = 0; i < ETHEREAL_MANAGER_TOKEN_SIZE; i++) {
		int hi = ethereal_hex_nibble(hex[i * 2]);
		int lo = ethereal_hex_nibble(hex[i * 2 + 1]);

		if (hi < 0 || lo < 0)
			return false;
		out[i] = (u8)((hi << 4) | lo);
		any |= out[i];
	}
	return hex[ETHEREAL_MANAGER_TOKEN_HEX_SIZE] == '\0' && any != 0;
}

static bool ethereal_manager_token_equal(
	const u8 left[ETHEREAL_MANAGER_TOKEN_SIZE],
	const u8 right[ETHEREAL_MANAGER_TOKEN_SIZE])
{
	volatile unsigned int diff = 0;
	unsigned int i;

	if (!left || !right)
		return false;
	for (i = 0; i < ETHEREAL_MANAGER_TOKEN_SIZE; i++)
		diff |= (unsigned int)(left[i] ^ right[i]);
	return diff == 0;
}

#endif
