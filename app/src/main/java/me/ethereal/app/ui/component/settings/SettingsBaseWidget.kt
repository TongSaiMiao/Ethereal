package me.ethereal.app.ui.component.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.material3.ripple
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.contentColorFor
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp

val LocalSegmentedItemShape = compositionLocalOf<Shape> { RoundedCornerShape(16.dp) }

@Composable
fun SettingsBaseWidget(
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    iconColor: Color? = null,
    iconPlaceholder: Boolean = true,
    title: String?,
    titleStyle: TextStyle = MaterialTheme.typography.titleMedium,
    description: String? = null,
    descriptionColor: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    descriptionStyle: TextStyle = MaterialTheme.typography.bodyMedium,
    enabled: Boolean = true,
    isError: Boolean = false,
    selected: Boolean = false,
    onClick: ((Offset) -> Unit)? = null,
    descriptionColumnContent: (@Composable ColumnScope.() -> Unit)? = null,
    containerColor: Color? = null,
    trailingContent: (@Composable BoxScope.(interactionSource: MutableInteractionSource) -> Unit)? = null,
) {
    val alpha = if (enabled) 1f else 0.38f
    val interactionSource = remember { MutableInteractionSource() }
    val density = LocalDensity.current
    val dynamicInternalPadding = (4 * density.fontScale).dp
    val baseShape = LocalSegmentedItemShape.current

    val backgroundColor = containerColor ?: if (selected) {
        MaterialTheme.colorScheme.primaryContainer
    } else {
        MaterialTheme.colorScheme.surfaceContainerHighest
    }

    val baseContentColor = if (selected) {
        MaterialTheme.colorScheme.contentColorFor(MaterialTheme.colorScheme.primaryContainer)
    } else {
        MaterialTheme.colorScheme.onSurface
    }

    val resolvedIconColor = iconColor ?: baseContentColor
    val finalDescriptionColor =
        if (isError) MaterialTheme.colorScheme.error else descriptionColor

    val colors = ListItemDefaults.colors(
        containerColor = backgroundColor,
        headlineColor = baseContentColor,
        leadingIconColor = resolvedIconColor,
        trailingIconColor = resolvedIconColor,
        supportingColor = finalDescriptionColor,
        disabledHeadlineColor = baseContentColor,
        disabledLeadingIconColor = resolvedIconColor,
        disabledTrailingIconColor = resolvedIconColor
    )

    val itemModifier = modifier
        .fillMaxWidth()
        .clip(baseShape)
        .then(
            if (onClick != null) {
                Modifier.clickable(
                    enabled = enabled,
                    interactionSource = interactionSource,
                    indication = ripple(),
                    onClick = { onClick(Offset.Zero) }
                )
            } else if (!enabled) {
                Modifier.semantics { disabled() }
            } else {
                Modifier
            }
        )

    val leading: (@Composable () -> Unit)? =
        if (icon == null && !iconPlaceholder) null
        else {
            {
                Box(
                    modifier = Modifier
                        .size(24.dp)
                        .alpha(alpha),
                    contentAlignment = Alignment.Center
                ) {
                    if (icon != null) {
                        Icon(
                            imageVector = icon,
                            contentDescription = null,
                            tint = resolvedIconColor
                        )
                    } else {
                        Spacer(modifier = Modifier.size(24.dp))
                    }
                }
            }
        }

    val supporting: (@Composable () -> Unit)? =
        if (description == null && descriptionColumnContent == null) null
        else {
            {
                Column {
                    description?.let { text ->
                        Text(
                            text = text,
                            style = descriptionStyle,
                            modifier = Modifier.alpha(alpha)
                        )
                    }
                    descriptionColumnContent?.invoke(this)
                    Spacer(Modifier.height(dynamicInternalPadding))
                }
            }
        }

    val trailing: (@Composable () -> Unit)? = trailingContent?.let { content ->
        {
            Box(
                modifier = Modifier.alpha(alpha),
                contentAlignment = Alignment.Center
            ) {
                content(interactionSource)
            }
        }
    }

    ListItem(
        headlineContent = {
            Box(
                modifier = Modifier
                    .alpha(alpha)
                    .padding(
                        top = dynamicInternalPadding,
                        bottom = if (description == null && descriptionColumnContent == null) {
                            dynamicInternalPadding
                        } else {
                            0.dp
                        }
                    )
            ) {
                title?.let {
                    Text(text = it, style = titleStyle)
                }
            }
        },
        modifier = itemModifier,
        colors = colors,
        leadingContent = leading,
        supportingContent = supporting,
        trailingContent = trailing
    )
}
