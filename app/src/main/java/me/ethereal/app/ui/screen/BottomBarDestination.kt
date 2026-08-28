package me.ethereal.app.ui.screen

import androidx.annotation.StringRes
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Apps
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.Apps
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Security
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.ui.graphics.vector.ImageVector
import com.ramcosta.composedestinations.generated.destinations.ModuleScreenDestination
import com.ramcosta.composedestinations.generated.destinations.HomeScreenDestination
import com.ramcosta.composedestinations.generated.destinations.SettingScreenDestination
import com.ramcosta.composedestinations.generated.destinations.SuperUserScreenDestination
import com.ramcosta.composedestinations.spec.DirectionDestinationSpec
import me.ethereal.app.R

enum class BottomBarDestination(
    val direction: DirectionDestinationSpec,
    @param:StringRes val label: Int,
    val iconSelected: ImageVector,
    val iconNotSelected: ImageVector,
    val kernelRequired: Boolean,
    val managerAccessRequired: Boolean,
    val serviceRequired: Boolean,
) {
    Home(
        HomeScreenDestination,
        R.string.home,
        Icons.Filled.Home,
        Icons.Outlined.Home,
        false,
        false,
        false
    ),
    SuperUser(
        SuperUserScreenDestination,
        R.string.su_title,
        Icons.Filled.Security,
        Icons.Outlined.Security,
        true,
        true,
        false
    ),
    Modules(
        ModuleScreenDestination,
        R.string.modules,
        Icons.Filled.Apps,
        Icons.Outlined.Apps,
        false,
        false,
        true
    ),
    Settings(
        SettingScreenDestination,
        R.string.settings,
        Icons.Filled.Settings,
        Icons.Outlined.Settings,
        false,
        false,
        false
    )
}

internal fun visibleBottomBarDestinations(
    kernelReady: Boolean,
    managerAccessReady: Boolean,
    serviceReady: Boolean,
): Set<BottomBarDestination> = BottomBarDestination.entries.filterTo(linkedSetOf()) { destination ->
    (!destination.kernelRequired || kernelReady) &&
        (!destination.managerAccessRequired || managerAccessReady) &&
        (!destination.serviceRequired || serviceReady)
}
