#include <assert.h>
#include <string.h>

#include "../ethinit/kmi_select.h"

static void check(const char *release, const char *major_minor,
		  const char *exact, int has_android_tag)
{
	struct ethereal_kmi_selection selection;

	ethereal_parse_kmi(release, &selection);
	assert(strcmp(selection.major_minor, major_minor) == 0);
	assert(strcmp(selection.exact, exact) == 0);
	assert(selection.has_android_tag == has_android_tag);
	assert(ethereal_kmi_allows_unique_major_minor(&selection) ==
	       (!has_android_tag && major_minor[0] != 0));
}

int main(void)
{
	check("5.10.218-android12-9-g123", "5.10", "android12-5.10", 1);
	check("5.4.210-android11-0-g123", "5.4", "android11-5.4", 1);
	check("5.15.153-oem", "5.15", "", 0);
	check("6.1.99", "6.1", "", 0);
	return 0;
}
