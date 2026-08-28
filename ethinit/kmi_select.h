#ifndef ETHEREAL_KMI_SELECT_H
#define ETHEREAL_KMI_SELECT_H

struct ethereal_kmi_selection {
	char major_minor[12];
	char exact[40];
	int has_android_tag;
};

static unsigned ethereal_kmi_len(const char *text)
{
	unsigned len = 0;

	if (text)
		while (text[len])
			len++;
	return len;
}

static int ethereal_kmi_starts_with(const char *text, const char *prefix)
{
	while (*prefix) {
		if (*text++ != *prefix++)
			return 0;
	}
	return 1;
}

static void ethereal_kmi_append(char *dest, unsigned capacity, const char *source)
{
	unsigned offset = ethereal_kmi_len(dest);
	unsigned index = 0;

	if (!source || offset >= capacity)
		return;
	while (source[index] && offset + index + 1 < capacity) {
		dest[offset + index] = source[index];
		index++;
	}
	dest[offset + index] = 0;
}

static void ethereal_parse_kmi(const char *release,
			      struct ethereal_kmi_selection *selection)
{
	unsigned index = 0;
	unsigned out = 0;
	int dots = 0;

	selection->major_minor[0] = 0;
	selection->exact[0] = 0;
	selection->has_android_tag = 0;
	while (release[index] &&
	       ((release[index] >= '0' && release[index] <= '9') ||
		release[index] == '.')) {
		if (index + 1 < sizeof(selection->major_minor))
			selection->major_minor[index] = release[index];
		index++;
	}
	if (index >= sizeof(selection->major_minor))
		index = sizeof(selection->major_minor) - 1;
	selection->major_minor[index] = 0;
	for (index = 0; selection->major_minor[index]; index++) {
		if (selection->major_minor[index] == '.' && ++dots == 2) {
			selection->major_minor[index] = 0;
			break;
		}
	}

	for (index = 0; release[index]; index++) {
		if (!ethereal_kmi_starts_with(release + index, "android") ||
		    release[index + 7] < '0' || release[index + 7] > '9')
			continue;
		selection->has_android_tag = 1;
		out = 0;
		while (release[index] && release[index] != '-' &&
		       out + 1 < sizeof(selection->exact))
			selection->exact[out++] = release[index++];
		selection->exact[out] = 0;
		ethereal_kmi_append(selection->exact, sizeof(selection->exact), "-");
		ethereal_kmi_append(selection->exact, sizeof(selection->exact),
				    selection->major_minor);
		return;
	}
}

static int ethereal_kmi_allows_unique_major_minor(
	const struct ethereal_kmi_selection *selection)
{
	return !selection->has_android_tag && selection->major_minor[0] != 0;
}

#endif
