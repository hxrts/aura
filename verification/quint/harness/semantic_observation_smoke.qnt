module semantic_observation_smoke {
  var phase: int

  action init = phase' = 0

  action launchActors = all { phase == 0, phase' = 1 }
  action onboardingVisible = all { phase == 1, phase' = 2 }
  action fillAccountName = all { phase == 2, phase' = 3 }
  action submitAccount = all { phase == 3, phase' = 4 }
  action neighborhoodVisible = all { phase == 4, phase' = 5 }
  action neighborhoodReady = all { phase == 5, phase' = 6 }
  action navigateSettings = all { phase == 6, phase' = 7 }
  action settingsVisible = all { phase == 7, phase' = 8 }

  action step = any {
    launchActors,
    onboardingVisible,
    fillAccountName,
    submitAccount,
    neighborhoodVisible,
    neighborhoodReady,
    navigateSettings,
    settingsVisible,
  }
}
