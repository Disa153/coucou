package com.coucou.android

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The app is in English and Russian (the phone's language decides); English is the source. */
class LanguagesTest {
    private fun read(path: String) = File(path).readText()
    private fun names(path: String) = Regex("""name="([a-z_0-9A-Z]+)"""").findAll(read(path)).map { it.groupValues[1] }.toList()

    @Test fun theBuildKeepsEnglishAndRussian() {
        assertTrue(Regex("""resourceConfigurations\s*\+=\s*listOf\("en", "ru"\)""").containsMatchIn(read("build.gradle.kts")))
    }

    @Test fun englishStringsAreTheSingleSourceOfTruth() {
        val source = read("../i18n/app-strings.json")
        val declared = Regex("""^\s{4}"([a-z_0-9A-Z]+)":""", RegexOption.MULTILINE).findAll(source.substringAfter(""""strings"""")).map { it.groupValues[1] }.toSet()
        assertEquals(declared, names("src/main/res/values/strings.xml").toSet())
    }

    @Test fun russianHasEveryStringAndNoOther() {
        assertEquals(names("src/main/res/values/strings.xml"), names("src/main/res/values-ru/strings.xml"))
    }

    @Test fun russianIsActuallyRussian() {
        val ru = read("src/main/res/values-ru/strings.xml")
        assertTrue(Regex("[а-яё]").findAll(ru).count() > 3000)
    }

    @Test fun onlyTheseLanguages() {
        val folders = File("src/main/res").listFiles().orEmpty().map { it.name }.filter { it.startsWith("values") }
        // values-notnight is not a language: it is the light variant of the Widgets' colours, and holds colours only.
        assertEquals(listOf("values", "values-notnight", "values-ru"), folders.sorted())
        assertEquals(listOf("colors.xml"), File("src/main/res/values-notnight").listFiles().orEmpty().map { it.name })
        assertEquals(listOf("strings.xml"), File("src/main/res/values-ru").listFiles().orEmpty().map { it.name })
        assertFalse(File("../i18n/extra.json").exists())
    }
}
