package me.ethereal.app.ui.component.settings

import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp

@Composable
fun SettingsJumpPageWidget(
    icon: ImageVector? = null,
    title: String,
    description: String? = null,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    SettingsBaseWidget(
        icon = icon,
        title = title,
        description = description,
        enabled = enabled,
        onClick = { onClick() }
    ) {
        Icon(
            imageVector = Icons.Filled.ChevronRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(24.dp)
        )
    }
}
