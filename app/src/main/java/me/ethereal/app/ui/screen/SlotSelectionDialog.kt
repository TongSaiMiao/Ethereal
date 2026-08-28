package me.ethereal.app.ui.screen

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SdStorage
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import me.ethereal.app.R
import me.ethereal.app.util.slotSuffix

@Composable
fun SlotSelectionDialog(
    show: Boolean,
    onDismiss: () -> Unit,
    onSlotSelected: (String) -> Unit,
) {
    var selectedSlot by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(show) {
        if (show) {
            val cur = slotSuffix(false).trimStart('_')
            selectedSlot = if (cur == "a" || cur == "b") cur else "a"
        }
    }

    if (!show) return
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.select_slot_title)) },
        text = {
            Column {
                listOf("a" to R.string.slot_a, "b" to R.string.slot_b).forEach { (id, label) ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(MaterialTheme.shapes.medium)
                            .clickable { selectedSlot = id }
                            .padding(vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.Start
                    ) {
                        RadioButton(
                            selected = selectedSlot == id,
                            onClick = { selectedSlot = id }
                        )
                        Icon(
                            Icons.Filled.SdStorage,
                            contentDescription = null,
                            modifier = Modifier
                                .padding(end = 8.dp)
                                .size(20.dp)
                        )
                        Text(stringResource(label))
                    }
                }
                Spacer(Modifier.height(4.dp))
            }
        },
        confirmButton = {
            TextButton(
                onClick = { selectedSlot?.let(onSlotSelected) },
                enabled = selectedSlot != null
            ) {
                Text(stringResource(android.R.string.ok))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text(stringResource(android.R.string.cancel))
            }
        }
    )
}
